use std::sync::Arc;

use chrono::Utc;
use entity::{challenges_subtasks, challenges_tasks, challenges_user_subtasks};
use lib::{auth::VerifiedUserAuth, config::Config, services::shop::AddCoinsError, SharedState};
use poem::web::Data;
use poem_ext::{db::DbTxn, response, responses::ErrorResponse};
use poem_openapi::{
    param::{Path, Query},
    payload::Json,
    OpenApi,
};
use schemas::challenges::subtasks::{Subtask, UserUpdateSubtaskRequest};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, ModelTrait, QueryFilter, Set,
    Unchanged,
};
use uuid::Uuid;

use super::Tags;
use crate::services::{
    subtasks::{
        get_user_subtask, query_subtasks_only, update_user_subtask, QuerySubtasksFilter,
        UserSubtaskExt,
    },
    tasks::{get_specific_task, Task},
};

mod bans;
mod config;
mod feedback;
mod reports;

#[derive(Clone)]
pub struct Subtasks {
    pub state: Arc<SharedState>,
    pub config: Arc<Config>,
}

impl Subtasks {
    pub fn get_api(self) -> impl OpenApi {
        (
            bans::Api,
            config::Api {
                config: Arc::clone(&self.config),
            },
            self.clone(),
            feedback::Api { state: self.state },
            reports::Api {
                config: self.config,
            },
        )
    }
}

#[OpenApi(tag = "Tags::Subtasks")]
impl Subtasks {
    /// List all subtasks across all parent tasks.
    #[allow(clippy::too_many_arguments)]
    #[oai(path = "/subtasks", method = "get")]
    pub async fn list_subtasks(
        &self,
        task_id: Query<Option<Uuid>>,
        /// Whether to search for free subtasks.
        free: Query<Option<bool>>,
        /// Whether to search for unlocked subtasks.
        unlocked: Query<Option<bool>>,
        /// Whether to search for subtasks the user has attempted to solve.
        attempted: Query<Option<bool>>,
        /// Whether to search for solved subtasks.
        solved: Query<Option<bool>>,
        /// Whether to search for rated subtasks.
        rated: Query<Option<bool>>,
        /// Whether to search for enabled subtasks.
        enabled: Query<Option<bool>>,
        /// Filter by creator.
        creator: Query<Option<Uuid>>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> ListSubtasks::Response<VerifiedUserAuth> {
        ListSubtasks::ok(
            query_subtasks_only(
                &db,
                &auth.0,
                task_id.0,
                QuerySubtasksFilter {
                    free: free.0,
                    unlocked: unlocked.0,
                    attempted: attempted.0,
                    solved: solved.0,
                    rated: rated.0,
                    enabled: enabled.0,
                    creator: creator.0,
                },
            )
            .await?,
        )
    }

    /// Unlock a subtask by paying its fee.
    #[oai(path = "/tasks/:task_id/subtasks/:subtask_id/access", method = "post")]
    pub async fn unlock_subtask(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> UnlockSubtask::Response<VerifiedUserAuth> {
        let Some((subtask, _)) = get_subtask(&db, task_id.0, subtask_id.0).await? else {
            return UnlockSubtask::subtask_not_found();
        };
        if !auth.0.admin && auth.0.id != subtask.creator && !subtask.enabled {
            return UnlockSubtask::subtask_not_found();
        }

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;
        if user_subtask.check_access(&auth.0, &subtask) {
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

        update_user_subtask(
            &db,
            user_subtask.as_ref(),
            challenges_user_subtasks::ActiveModel {
                user_id: Set(auth.0.id),
                subtask_id: Set(subtask.id),
                unlocked_timestamp: Set(Some(Utc::now().naive_utc())),
                ..Default::default()
            },
        )
        .await?;

        UnlockSubtask::unlocked()
    }

    /// Update a subtask.
    #[oai(path = "/tasks/:task_id/subtasks/:subtask_id", method = "patch")]
    pub async fn update_subtask(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        data: Json<UserUpdateSubtaskRequest>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> UpdateSubtask::Response {
        let Some((subtask, task)) = get_subtask(&db, task_id.0, subtask_id.0).await? else {
            return UpdateSubtask::subtask_not_found();
        };
        if !auth.0.admin && auth.0.id != subtask.creator {
            return UpdateSubtask::permission_denied();
        }

        let Some(specific) = get_specific_task(&db, &task).await? else {
            return UpdateSubtask::subtask_not_found();
        };
        if matches!(specific, Task::CourseTask(_))
            && *data.0.fee.get_new(&(subtask.fee as _)) > self.config.challenges.quizzes.max_fee
            && !auth.0.admin
        {
            return UpdateSubtask::fee_limit_exceeded(self.config.challenges.quizzes.max_fee);
        }

        challenges_subtasks::ActiveModel {
            id: Unchanged(subtask.id),
            task_id: Unchanged(subtask.task_id),
            ty: Unchanged(subtask.ty),
            creator: Unchanged(subtask.creator),
            creation_timestamp: Unchanged(subtask.creation_timestamp),
            xp: Unchanged(subtask.xp),
            coins: Unchanged(subtask.coins),
            fee: data.0.fee.map(|x| x as _).update(subtask.fee),
            enabled: Unchanged(subtask.enabled),
        }
        .update(&***db)
        .await?;

        UpdateSubtask::ok()
    }

    /// Delete a subtask.
    #[oai(path = "/tasks/:task_id/subtasks/:subtask_id", method = "delete")]
    async fn delete_question(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> DeleteSubtask::Response<VerifiedUserAuth> {
        let Some(subtask) = challenges_subtasks::Entity::find_by_id(subtask_id.0)
            .filter(challenges_subtasks::Column::TaskId.eq(task_id.0))
            .one(&***db)
            .await?
        else {
            return DeleteSubtask::subtask_not_found();
        };

        if !(auth.0.admin || auth.0.id == subtask.creator) {
            return DeleteSubtask::forbidden();
        }

        subtask.delete(&***db).await?;
        DeleteSubtask::ok()
    }
}

response!(ListSubtasks = {
    Ok(200) => Vec<Subtask>,
});

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

response!(UpdateSubtask = {
    Ok(200),
    /// The subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user is not allowed to modify this subtask.
    PermissionDenied(403, error),
    /// The max fee limit has been exceeded.
    FeeLimitExceeded(403, error) => u64,
});

response!(DeleteSubtask = {
    Ok(200),
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user is not allowed to delete this subtask.
    Forbidden(403, error),
});

async fn get_subtask(
    db: &DatabaseTransaction,
    task_id: Uuid,
    subtask_id: Uuid,
) -> Result<Option<(challenges_subtasks::Model, challenges_tasks::Model)>, ErrorResponse> {
    Ok(
        match challenges_subtasks::Entity::find_by_id(subtask_id)
            .find_also_related(challenges_tasks::Entity)
            .filter(challenges_subtasks::Column::TaskId.eq(task_id))
            .one(db)
            .await?
        {
            Some((subtask, Some(task))) => Some((subtask, task)),
            _ => None,
        },
    )
}
