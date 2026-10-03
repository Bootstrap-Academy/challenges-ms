use fnct::key;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Service, ServiceResult};

#[derive(Debug, Clone)]
pub struct AuthService(Service);

/// Part of the cache key of a cached [`User`].
///
/// The cache format is not self-describing, so an entry that was written before
/// a field was added to [`User`] cannot be decoded into the current struct.
/// Increasing this retires those entries instead of letting them fail; they
/// expire on their own.
const USER_CACHE_VERSION: u32 = 1;

impl AuthService {
    pub(super) fn new(service: Service) -> Self {
        Self(service)
    }

    pub async fn get_user_by_id(&self, id: Uuid) -> ServiceResult<Option<User>> {
        self.0
            .cache
            .cached_result(
                key!(USER_CACHE_VERSION, id),
                &[&format!("{id}")],
                None,
                || self.get_user_by_id_uncached(id),
            )
            .await?
    }

    /// Minimum case-specific provenance. This is deliberately uncached and
    /// never treats an account's recorded version as a legal-capacity finding.
    pub async fn moderation_basis(&self, id: Uuid) -> ServiceResult<serde_json::Value> {
        let response = self
            .0
            .get(&format!("/moderation/rule-evidence/{id}"))
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(
                serde_json::json!({"recorded_acceptance":"unknown","automatic_quality_basis_confirmed":false}),
            );
        }
        Ok(response.error_for_status()?.json().await?)
    }

    /// Fresh ordinary authority, separate from the internal identity endpoint.
    pub async fn ordinary_authority(
        &self,
        token: &str,
    ) -> ServiceResult<Option<OrdinaryAuthority>> {
        let response = self
            .0
            .post("/ordinary-authority")
            .json(&serde_json::json!({"access_token":token}))
            .send()
            .await?;
        if response.status() == StatusCode::UNAUTHORIZED
            || response.status() == StatusCode::FORBIDDEN
        {
            return Ok(None);
        }
        Ok(Some(response.error_for_status()?.json().await?))
    }

    /// Same as [`get_user_by_id`](Self::get_user_by_id), but always asks the
    /// auth microservice instead of using the cache.
    pub async fn get_user_by_id_uncached(&self, id: Uuid) -> ServiceResult<Option<User>> {
        match self
            .0
            .get(&format!("/users/{id}"))
            .send()
            .await?
            .error_for_status()
        {
            Ok(resp) => Ok(Some(resp.json().await?)),
            Err(err) if err.status() == Some(StatusCode::NOT_FOUND) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
    pub registration: f64,
    pub admin: bool,
    /// Whether the user asked not to be listed on the leaderboards. Older
    /// versions of the auth microservice do not send this field.
    #[serde(default)]
    pub leaderboard_opt_out: bool,
}

#[derive(Deserialize)]
pub struct OrdinaryAuthority {
    pub id: Uuid,
    pub admin: bool,
    pub email_verified: bool,
}
