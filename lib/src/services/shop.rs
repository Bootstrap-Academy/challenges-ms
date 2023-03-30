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
