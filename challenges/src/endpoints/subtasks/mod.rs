use std::sync::Arc;

use entity::{challenges_subtasks, challenges_tasks, sea_orm_active_enums::ChallengesSubtaskType};
use lib::{auth::VerifiedUserAuth, config::Config, SharedState};
use poem::web::Data;
use poem_ext::{db::DbTxn, response, responses::ErrorResponse};
use poem_openapi::{
    param::{Path, Query},
    OpenApi,
};
use schemas::challenges::subtasks::{Subtask, SubtaskStats};
use sea_orm::{ColumnTrait, DatabaseTransaction, EntityTrait, ModelTrait, QueryFilter};
use uuid::Uuid;

use super::Tags;
use crate::services::subtasks::{
    get_user_subtasks, query_subtasks_only, stat_subtasks, stat_subtasks_prepare,
    QuerySubtasksFilter,
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
            feedback::Api {
                state: Arc::clone(&self.state),
                config: Arc::clone(&self.config),
            },
            reports::Api {
                state: Arc::clone(&self.state),
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
        /// Filter by subtask type.
        subtask_type: Query<Option<ChallengesSubtaskType>>,
        /// Whether to search for subtasks the user has attempted to solve.
        attempted: Query<Option<bool>>,
        /// Whether to search for solved subtasks.
        solved: Query<Option<bool>>,
        /// Whether to search for rated subtasks.
        rated: Query<Option<bool>>,
        /// Whether to search for enabled subtasks.
        enabled: Query<Option<bool>>,
        /// Whether to search for retired subtasks.
        retired: Query<Option<bool>>,
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
                    attempted: attempted.0,
                    solved: solved.0,
                    rated: rated.0,
                    enabled: enabled.0,
                    retired: retired.0,
                    creator: creator.0,
                    ty: subtask_type.0,
                },
            )
            .await?,
        )
    }

    /// Return user specific subtask statistics
    #[oai(path = "/subtasks/stats", method = "get")]
    pub async fn get_subtask_stats(
        &self,
        task_id: Query<Option<Uuid>>,
        /// Filter by subtask type.
        subtask_type: Query<Option<ChallengesSubtaskType>>,
        /// Filter by creator.
        creator: Query<Option<Uuid>>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetSubtaskStats::Response<VerifiedUserAuth> {
        let mut filter = QuerySubtasksFilter {
            creator: creator.0,
            ty: subtask_type.0,
            ..Default::default()
        };

        let user_subtasks = get_user_subtasks(&db, auth.0.id).await?;
        let subtasks =
            stat_subtasks_prepare(&db, &auth.0, task_id.0.map(|x| vec![x]), &filter).await?;

        filter.ty = None;
        GetSubtaskStats::ok(stat_subtasks(&subtasks, &user_subtasks, filter))
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

        if auth.0.id != subtask.creator {
            return DeleteSubtask::forbidden();
        }

        crate::services::moderation::lock_subtask(&db, subtask.id).await?;
        crate::services::moderation::erasure_marker(&db, auth.0.id).await?;
        subtask.delete(&***db).await?;
        DeleteSubtask::ok()
    }

    /// Scoped retained learning only; no ordinary session or publication authority.
    #[oai(path = "/learning/subtasks", method = "get")]
    #[allow(clippy::too_many_arguments)]
    async fn learning_list_subtasks(
        &self,
        task_id: Query<Option<Uuid>>,
        subtask_type: Query<Option<ChallengesSubtaskType>>,
        attempted: Query<Option<bool>>,
        solved: Query<Option<bool>>,
        rated: Query<Option<bool>>,
        enabled: Query<Option<bool>>,
        retired: Query<Option<bool>>,
        creator: Query<Option<Uuid>>,
        db: Data<&DbTxn>,
        auth: lib::auth::LearningAuth,
    ) -> ListSubtasks::Response<VerifiedUserAuth> {
        let user = crate::services::learning::admit(&db, &self.state.services, auth.0).await?;
        // Reuse product behavior after dedicated scoped admission. This local
        // wrapper value does not pass through any ordinary HTTP authenticator.
        self.list_subtasks(
            task_id,
            subtask_type,
            attempted,
            solved,
            rated,
            enabled,
            retired,
            creator,
            db,
            VerifiedUserAuth(user),
        )
        .await
    }

    /// Scoped retained learning only; no ordinary session or publication authority.
    #[oai(path = "/learning/subtasks/stats", method = "get")]
    #[allow(clippy::too_many_arguments)]
    async fn learning_get_subtask_stats(
        &self,
        task_id: Query<Option<Uuid>>,
        subtask_type: Query<Option<ChallengesSubtaskType>>,
        creator: Query<Option<Uuid>>,
        db: Data<&DbTxn>,
        auth: lib::auth::LearningAuth,
    ) -> GetSubtaskStats::Response<VerifiedUserAuth> {
        let user = crate::services::learning::admit(&db, &self.state.services, auth.0).await?;
        // Reuse product behavior after dedicated scoped admission. This local
        // wrapper value does not pass through any ordinary HTTP authenticator.
        self.get_subtask_stats(task_id, subtask_type, creator, db, VerifiedUserAuth(user))
            .await
    }
}

response!(ListSubtasks = {
    Ok(200) => Vec<Subtask>,
});

response!(GetSubtaskStats = {
    Ok(200) => SubtaskStats,
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
            Some((subtask, Some(task))) => Some((
                crate::services::moderation::effective_subtask(db, subtask).await?,
                task,
            )),
            _ => None,
        },
    )
}
