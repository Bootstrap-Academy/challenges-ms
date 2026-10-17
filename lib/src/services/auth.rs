use std::time::Duration;

use fnct::key;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Service, ServiceError, ServiceResult};

#[derive(Debug, Clone)]
pub struct AuthService(pub(super) Service);

/// How long a user lookup waits for the auth microservice, like the other
/// internal calls.
pub const USER_LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);

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
    ///
    /// Returns [`None`] only if the auth microservice says that the user does
    /// not exist. Any other `404`, such as the one for a route that does not
    /// exist, is an error, because it says nothing about the user.
    ///
    /// Gives up after [`USER_LOOKUP_TIMEOUT`], so an auth microservice that
    /// accepts the connection and never answers cannot hold the deleted user
    /// sweep, a leaderboard or a moderation request indefinitely.
    pub async fn get_user_by_id_uncached(&self, id: Uuid) -> ServiceResult<Option<User>> {
        let response = self
            .0
            .get(&format!("/users/{id}"))
            .timeout(USER_LOOKUP_TIMEOUT)
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return if is_user_not_found(&response.bytes().await?) {
                Ok(None)
            } else {
                Err(ServiceError::UnexpectedStatusCode(StatusCode::NOT_FOUND))
            };
        }
        Ok(Some(response.error_for_status()?.json().await?))
    }
}

/// Whether a `404` body is the `UserNotFoundError` of the auth microservice,
/// `{"detail": "User not found"}`.
fn is_user_not_found(body: &[u8]) -> bool {
    #[derive(Deserialize)]
    struct ErrorBody {
        detail: String,
    }
    serde_json::from_slice::<ErrorBody>(body).is_ok_and(|body| body.detail == "User not found")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_not_found_error_means_the_user_is_gone() {
        // `ApiError<UserNotFoundError>` of the backend, as it answers
        // `GET /auth/_internal/users/{user_id}` for an unknown user.
        assert!(is_user_not_found(br#"{"detail":"User not found"}"#));
        assert!(is_user_not_found(
            br#"{"detail": "User not found", "code": "user_not_found"}"#
        ));
    }

    #[test]
    fn test_other_not_found_bodies_say_nothing_about_the_user() {
        for body in [
            // axum without a fallback, for a route that does not exist
            &b""[..],
            // the default of FastAPI and Starlette for a route that does not exist
            br#"{"detail":"Not Found"}"#,
            br#"{"detail":"Invoice not found"}"#,
            br#"{"detail":"user not found"}"#,
            br#"{"error":"User not found"}"#,
            b"<html><head><title>404 Not Found</title></head></html>",
            b"User not found",
        ] {
            assert!(
                !is_user_not_found(body),
                "{}",
                String::from_utf8_lossy(body)
            );
        }
    }
}
