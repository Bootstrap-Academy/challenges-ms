//! Outcome facts and debit commands commit together. Only committed commands
//! are delivered; every remote retry carries the exact original attempt UUID.
use std::sync::{Arc, Mutex};

use lib::{auth::User, services::Services, SharedState};
use poem::{Endpoint, IntoResponse, Middleware, Request, Response};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, DbErr, Statement,
    TransactionTrait,
};
use serde_json::{json, Value};
use uuid::Uuid;

pub const WRONG_ANSWER_HALF_HEARTS: u32 = 2;

/// None refuses admission; Some(true) preserves an existing fee exemption.
pub async fn admit(
    services: &Services,
    user: &User,
    subtask: &entity::challenges_subtasks::Model,
) -> anyhow::Result<Option<bool>> {
    if subtask.retired
        || user.admin
        || user.id == subtask.creator
        || services.shop.has_premium(user.id).await?
    {
        return Ok(Some(true));
    }
    Ok((services.shop.get_hearts(user.id).await? >= WRONG_ANSWER_HALF_HEARTS).then_some(false))
}

#[derive(Clone, Default)]
pub struct PendingHeartOperations(Arc<Mutex<Vec<Uuid>>>);

impl PendingHeartOperations {
    pub fn add(&self, operation: Uuid) {
        self.0
            .lock()
            .expect("heart settlement queue")
            .push(operation);
    }
}

pub async fn record(
    db: &DatabaseTransaction,
    operation: Uuid,
    user: Uuid,
    subtask: Uuid,
) -> Result<(), DbErr> {
    db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO challenge_heart_operations(id,user_id,subtask_id) VALUES($1,$2,$3) ON CONFLICT(id) DO NOTHING",
        [operation.into(), user.into(), subtask.into()])).await?;
    Ok(())
}

pub async fn settle(
    db: &DatabaseConnection,
    services: &Services,
    operation: Uuid,
) -> anyhow::Result<bool> {
    settle_with(db, operation, |user| {
        services.shop.apply_heart_operation(operation, user)
    })
    .await
}

async fn settle_with<F, Fut>(
    db: &DatabaseConnection,
    operation: Uuid,
    deliver: F,
) -> anyhow::Result<bool>
where
    F: FnOnce(Uuid) -> Fut,
    Fut: std::future::Future<Output = lib::services::ServiceResult<Value>>,
{
    let tx = db.begin().await?;
    let Some(subject) = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT user_id FROM challenge_heart_operations WHERE id=$1",
            [operation.into()],
        ))
        .await?
    else {
        return Ok(true);
    };
    let user: Uuid = subject.try_get("", "user_id")?;
    // Same lock order as account erasure: subject first, then the operation.
    super::benefits::lock_subject(&tx, user).await?;
    let Some(row) = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT state FROM challenge_heart_operations WHERE id=$1 FOR UPDATE",
            [operation.into()],
        ))
        .await?
    else {
        return Ok(true);
    };
    let state: String = row.try_get("", "state")?;
    if state != "pending" {
        return Ok(state == "settled");
    }
    let (state, receipt) = match deliver(user).await {
        Ok(receipt) if receipt["outcome"] == "conflict" => ("review", receipt),
        Ok(receipt) => ("settled", receipt),
        Err(_) => ("pending", json!({"outcome":"pending"})),
    };
    tx.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "UPDATE challenge_heart_operations SET state=$2,receipt=$3,attempts=attempts+1,next_attempt_at=clock_timestamp()+interval '5 seconds' WHERE id=$1",
        [operation.into(), state.into(), receipt.into()])).await?;
    tx.commit().await?;
    Ok(state == "settled")
}

pub async fn settle_user(
    db: &DatabaseConnection,
    services: &Services,
    user: Uuid,
) -> anyhow::Result<()> {
    let rows = db.query_all(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT id FROM challenge_heart_operations WHERE user_id=$1 AND state='pending' ORDER BY created_at,id LIMIT 20", [user.into()])).await?;
    for row in rows {
        settle(db, services, row.try_get("", "id")?).await?;
    }
    Ok(())
}

pub async fn unsettled_user(
    db: &DatabaseTransaction,
    user: Uuid,
) -> Result<std::collections::HashSet<Uuid>, DbErr> {
    db.query_all(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT id FROM challenge_heart_operations WHERE user_id=$1 AND state<>'settled'",
        [user.into()],
    ))
    .await?
    .into_iter()
    .map(|row| row.try_get("", "id"))
    .collect()
}

pub async fn run(db: DatabaseConnection, services: Services) {
    loop {
        let pass = async {
            let rows = db.query_all(Statement::from_string(DbBackend::Postgres,
                "SELECT id FROM challenge_heart_operations WHERE state='pending' AND next_attempt_at<=clock_timestamp() ORDER BY next_attempt_at,id LIMIT 100".to_owned())).await?;
            for row in rows { settle(&db, &services, row.try_get("", "id")?).await?; }
            anyhow::Ok(())
        }.await;
        if let Err(err) = pass {
            tracing::warn!("Heart settlement remains pending: {err}");
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

/// Wrap the DB middleware, so the outbox is committed before any shop mutation.
pub struct SettlementMiddleware(pub Arc<SharedState>);
pub struct SettlementEndpoint<E> {
    inner: E,
    state: Arc<SharedState>,
}
impl<E: Endpoint> Middleware<E> for SettlementMiddleware {
    type Output = SettlementEndpoint<E>;
    fn transform(&self, inner: E) -> Self::Output {
        SettlementEndpoint {
            inner,
            state: self.0.clone(),
        }
    }
}
impl<E: Endpoint> Endpoint for SettlementEndpoint<E> {
    type Output = Response;
    async fn call(&self, mut req: Request) -> poem::Result<Response> {
        let pending = PendingHeartOperations::default();
        req.extensions_mut().insert(pending.clone());
        let mut response = self.inner.call(req).await?.into_response();
        let operations = pending.0.lock().expect("heart settlement queue").clone();
        if response.status().is_success() && !operations.is_empty() {
            let mut unresolved = false;
            for operation in operations {
                if !matches!(
                    settle(&self.state.db, &self.state.services, operation).await,
                    Ok(true)
                ) {
                    unresolved = true;
                }
            }
            if unresolved {
                // Keep the accepted result; a transport failure is not a new
                // attempt and must not encourage a repeated paid POST.
                let body = response.take_body().into_json::<Value>().await?;
                let mut body = body;
                body["hearts_pending"] = json!(true);
                response.set_body(serde_json::to_vec(&body).expect("JSON attempt response"));
            }
        }
        Ok(response)
    }
}
