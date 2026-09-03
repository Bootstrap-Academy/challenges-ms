use fnct::key;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Service, ServiceResult};

#[derive(Debug, Clone)]
pub struct AuthService(Service);

impl AuthService {
    pub(super) fn new(service: Service) -> Self {
        Self(service)
    }

    pub async fn get_user_by_id(&self, id: Uuid) -> ServiceResult<Option<User>> {
        self.0
            .cache
            .cached_result(key!(id), &[&format!("{id}")], None, || {
                self.get_user_by_id_uncached(id)
            })
            .await?
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
}
