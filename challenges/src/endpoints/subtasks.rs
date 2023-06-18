use std::sync::Arc;

use chrono::Utc;
use entity::{challenges_subtasks, challenges_unlocked_subtasks};
use lib::{auth::VerifiedUserAuth, config::Config, services::shop::AddCoinsError, SharedState};
use poem::web::Data;
use poem_ext::{db::DbTxn, response, responses::ErrorResponse};
use poem_openapi::{param::Path, OpenApi};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, ModelTrait, QueryFilter, Set,
};
use uuid::Uuid;

use super::Tags;

pub struct Subtasks {
    pub state: Arc<SharedState>,
    pub config: Arc<Config>,
}

#[OpenApi(tag = "Tags::Subtasks")]
impl Subtasks {
    /// Unlock a subtask by paying its fee.
    #[oai(path = "/tasks/:task_id/subtasks/:subtask_id/access", method = "post")]
    pub async fn unlock_subtask(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> UnlockSubtask::Response<VerifiedUserAuth> {
        let Some(subtask) = get_subtask(&db, task_id.0, subtask_id.0).await? else {
            return UnlockSubtask::subtask_not_found();
        };

        if subtask.fee <= 0 {
            return UnlockSubtask::ok();
        }

        if subtask
            .find_related(challenges_unlocked_subtasks::Entity)
            .filter(challenges_unlocked_subtasks::Column::UserId.eq(auth.0.id))
            .one(&***db)
            .await?
            .is_some()
        {
            return UnlockSubtask::ok();
        }

        match self
            .state
            .services
            .shop
            .add_coins(auth.0.id, -subtask.fee, "Quiz", false)
            .await?
        {
            Ok(_) => {}
            Err(AddCoinsError::NotEnoughCoins) => {
                return UnlockSubtask::not_enough_coins();
            }
        }

        challenges_unlocked_subtasks::ActiveModel {
            user_id: Set(auth.0.id),
            subtask_id: Set(subtask.id),
            timestamp: Set(Utc::now().naive_utc()),
        }
        .insert(&***db)
        .await?;

        UnlockSubtask::unlocked()
    }
}

response!(UnlockSubtask = {
    /// The user already has access to this subtask.
    Ok(200),
    /// The fee has been paid and the subtask has been unlocked successfully.
    Unlocked(201),
    /// The subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user does not have enough coins to pay the fee.
    NotEnoughCoins(403, error),
});

async fn get_subtask(
    db: &DatabaseTransaction,
    task_id: Uuid,
    subtask_id: Uuid,
) -> Result<Option<challenges_subtasks::Model>, ErrorResponse> {
    Ok(challenges_subtasks::Entity::find_by_id(subtask_id)
        .filter(challenges_subtasks::Column::TaskId.eq(task_id))
        .one(db)
        .await?)
}
