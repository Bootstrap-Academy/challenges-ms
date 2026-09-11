use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use super::{Service, ServiceResult};

#[derive(Debug, Clone)]
pub struct ShopService(Service);

impl ShopService {
    pub async fn learning_authority(
        &self,
        digest: &str,
    ) -> ServiceResult<Option<crate::auth::User>> {
        let response = self
            .0
            .post("/claims/learning_authority_digest")
            .json(&serde_json::json!({"hash":digest}))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;
        if matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            return Ok(None);
        }
        if response.status() != StatusCode::OK {
            return Err(super::ServiceError::UnexpectedStatusCode(response.status()));
        }
        let value: serde_json::Value = response.json().await?;
        if value.is_null() {
            return Ok(None);
        }
        if value["purpose"] != "retained_learning"
            || value["ordinary_authority"] != false
            || value["financial_authority"] != false
            || value["admin"] != false
            || value["email_verified"] != true
        {
            return Err(super::ServiceError::MalformedResponse(
                "Invalid limited learning authority",
            ));
        }
        let id = value["subject"]
            .as_str()
            .and_then(|v| Uuid::parse_str(v).ok())
            .ok_or(super::ServiceError::MalformedResponse(
                "Missing limited learning subject",
            ))?;
        Ok(Some(crate::auth::User {
            id,
            email_verified: true,
            admin: false,
        }))
    }

    pub async fn apply_benefit(
        &self,
        operation: Uuid,
        user: Uuid,
        request: &serde_json::Value,
    ) -> ServiceResult<serde_json::Value> {
        // This producer dispatches earned credits, never a new unapproved debit.
        if request["coins"].as_i64().is_none_or(|coins| coins < 0) {
            return Ok(
                serde_json::json!({"state":"review","reason":"Original configured benefit is not a nonnegative credit"}),
            );
        }
        let response = self
            .0
            .put(&format!("/coin-operations/{operation}/{user}"))
            .json(request)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(unavailable_coin_recipient(operation));
        }
        if response.status() == StatusCode::CONFLICT {
            return Ok(
                serde_json::json!({"state":"review","reason":"Exact coin receipt conflict"}),
            );
        }
        if response.status() != StatusCode::OK {
            return Err(super::ServiceError::UnexpectedStatusCode(response.status()));
        }
        let result: serde_json::Value = response.json().await?;
        if !valid_committed_balance(&result) {
            return Ok(
                serde_json::json!({"state":"uncertain","reason":"Unrecognized committed coin result"}),
            );
        }
        Ok(
            serde_json::json!({"state":"applied","operation_id":operation,"request":request,"balance":result,
            "ledger_id":null,"ledger_link":"not supplied by the original keyed API"}),
        )
    }

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
        // This result authorizes actual attempts. An older cached negative
        // must not deny or charge hearts after committed Premium activation.
        Ok(self
            .0
            .get(&format!("/premium/{user_id}"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn get_hearts(&self, user_id: Uuid) -> ServiceResult<u32> {
        // Example judging uses this balance as an admission condition too.
        // Read the authority afresh; deductions remain atomic at the backend.
        Ok(self
            .0
            .get(&format!("/hearts/{user_id}"))
            .send()
            .await?
            .error_for_status()?
            .json::<Hearts>()
            .await?
            .hearts)
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

// Match the actual backend ApiBalance schema. A syntactically numeric response
// is not an applied receipt if either unsigned balance is missing or malformed.
fn valid_committed_balance(value: &serde_json::Value) -> bool {
    value["coins"].as_u64().is_some() && value["withheld_coins"].as_u64().is_some()
}

fn unavailable_coin_recipient(operation: Uuid) -> serde_json::Value {
    // The endpoint's joined-recipient lookup and an unverified route response
    // do not independently establish a literal erasure or an earlier outcome.
    serde_json::json!({"state":"uncertain","operation_id":operation,"http_status":404,
        "reason":"Recipient or route unavailable; retry the original operation","applied":null})
}

#[cfg(test)]
mod benefit_receipt_tests {
    use super::{unavailable_coin_recipient, valid_committed_balance};
    use serde_json::json;

    #[test]
    fn actual_unsigned_balance_schema_only() {
        for value in [
            json!({"coins":0,"withheld_coins":0}),
            json!({"coins":73,"withheld_coins":5}),
            json!({"coins":u64::MAX,"withheld_coins":0}),
        ] {
            assert!(valid_committed_balance(&value));
        }
        for value in [
            json!({"coins":-1,"withheld_coins":0}),
            json!({"coins":0,"withheld_coins":-1}),
            json!({"coins":1.5,"withheld_coins":0}),
            json!({"coins":"1","withheld_coins":0}),
            json!({"coins":true,"withheld_coins":0}),
            json!({"coins":0}),
            json!(null),
        ] {
            assert!(!valid_committed_balance(&value));
        }
    }

    #[test]
    fn remote_not_found_is_unknown_and_keeps_original_operation() {
        let operation = uuid::Uuid::new_v4();
        let value = unavailable_coin_recipient(operation);
        assert_eq!(value["state"], "uncertain");
        assert_eq!(value["operation_id"], json!(operation));
        assert_eq!(value["http_status"], 404);
        assert!(value["applied"].is_null());
    }
}
