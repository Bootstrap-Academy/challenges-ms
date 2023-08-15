use std::time::Duration;

use fnct::key;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use super::{Service, ServiceResult};

#[derive(Debug, Clone)]
pub struct ShopService(Service);

impl ShopService {
    pub(super) fn new(service: Service) -> Self {
        Self(service)
    }

    pub async fn add_coins(
        &self,
        user_id: Uuid,
        coins: i64,
        description: &str,
        credit_note: bool,
    ) -> ServiceResult<Result<Balance, AddCoinsError>> {
        let response = self
            .0
            .post(&format!("/coins/{user_id}"))
            .json(&AddCoinsRequest {
                coins,
                description,
                credit_note,
            })
            .send()
            .await?;
        Ok(match response.status() {
            StatusCode::OK => Ok(response.json().await?),
            StatusCode::PRECONDITION_FAILED => Err(AddCoinsError::NotEnoughCoins),
            code => return Err(super::ServiceError::UnexpectedStatusCode(code)),
        })
    }

    pub async fn has_premium(&self, user_id: Uuid) -> ServiceResult<bool> {
        Ok(self
            .0
            .cache
            .cached_result(
                key!(user_id),
                &[],
                Some(Duration::from_secs(10)),
                || async {
                    self.0
                        .get(&format!("/premium/{user_id}"))
                        .send()
                        .await?
                        .error_for_status()?
                        .json()
                        .await
                },
            )
            .await??)
    }

    pub async fn get_hearts(&self, user_id: Uuid) -> ServiceResult<u32> {
        Ok(self
            .0
            .cache
            .cached_result(
                key!(user_id),
                &["hearts", &format!("{user_id}")],
                Some(Duration::from_secs(10)),
                || async {
                    Ok::<_, reqwest::Error>(
                        self.0
                            .get(&format!("/hearts/{user_id}"))
                            .send()
                            .await?
                            .error_for_status()?
                            .json::<Hearts>()
                            .await?
                            .hearts,
                    )
                },
            )
            .await??)
    }

    pub async fn add_hearts(&self, user_id: Uuid, hearts: i32) -> ServiceResult<bool> {
        let success = self
            .0
            .post(&format!("/hearts/{user_id}"))
            .json(&AddHeartsRequest { hearts })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if success {
            self.0
                .cache
                .pop_tags(&["hearts", &format!("{user_id}")])
                .await?;
        }
        Ok(success)
    }
}

#[derive(Debug, Deserialize)]
pub struct Balance {
    pub coins: i64,
    pub withheld_coins: i64,
}

#[derive(Debug, Error)]
pub enum AddCoinsError {
    #[error("Not enough coins")]
    NotEnoughCoins,
}

#[derive(Debug, Serialize)]
struct AddCoinsRequest<'a> {
    coins: i64,
    description: &'a str,
    credit_note: bool,
}

#[derive(Debug, Deserialize)]
struct Hearts {
    hearts: u32,
}

#[derive(Debug, Serialize)]
struct AddHeartsRequest {
    hearts: i32,
}
