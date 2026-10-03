use std::sync::Arc;

use lib::{auth::InternalAuth, config::Config, SharedState};
use poem::web::Data;
use poem_ext::{db::DbTxn, response, responses::Response};
use poem_openapi::{param::Path, payload::Json, ApiResponse, OpenApi};
use schemas::challenges::{
    learning_history::{LearningHistory, LearningHistoryRequest},
    lesson_milestones::{RecordLessonMilestoneRequest, RecordedLessonMilestone},
    user_export::UserDataExport,
};
use sea_orm::ConnectionTrait;
use tracing::info;
use uuid::Uuid;

use super::Tags;
use crate::services::{
    lesson_milestones::{self, Recorded},
    users::{delete_user_data, export_user_data},
};

pub struct Internal {
    pub state: Arc<SharedState>,
    pub config: Arc<Config>,
}

#[OpenApi(tag = "Tags::Internal")]
impl Internal {
    /// Read existing participation in requested exercises and exact lectures.
    ///
    /// Both lists default to empty; at most 500 combined entries are accepted.
    /// This does not grant success, alter progress, or call another service.
    #[oai(path = "/_internal/users/:user_id/learning-history", method = "post")]
    async fn learning_history(
        &self,
        user_id: Path<Uuid>,
        db: Data<&DbTxn>,
        _auth: InternalAuth,
        data: Json<LearningHistoryRequest>,
    ) -> GetLearningHistory::Response<InternalAuth> {
        if data.0.subtask_ids.len() + data.0.lecture_bindings.len() > 500 {
            return GetLearningHistory::too_many_entries();
        }
        db.execute_unprepared("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .await?;
        GetLearningHistory::ok(
            crate::services::learning_history::lookup(&***db, user_id.0, data.0).await?,
        )
    }

    /// Return all data that belongs to a user.
    ///
    /// The export is empty for a user without any data in this service.
    #[oai(path = "/_internal/users/:user_id/export", method = "get")]
    async fn export_user(
        &self,
        user_id: Path<Uuid>,
        db: Data<&DbTxn>,
        _auth: InternalAuth,
    ) -> ExportUser::Response<InternalAuth> {
        // Definitions and ownership must come from one snapshot, including
        // while another request edits or deletes the authored content.
        db.execute_unprepared("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .await?;
        ExportUser::ok(export_user_data(&db, user_id.0).await?)
    }

    /// Delete all data that belongs to a user.
    ///
    /// This endpoint is idempotent, deleting a user without any data is a
    /// success.
    #[oai(path = "/_internal/users/:user_id", method = "delete")]
    async fn delete_user(
        &self,
        user_id: Path<Uuid>,
        db: Data<&DbTxn>,
        _auth: InternalAuth,
    ) -> Response<DeleteUser, InternalAuth> {
        let rows = delete_user_data(&db, user_id.0).await?;
        self.state.cache.pop_tag(&format!("{}", user_id.0)).await?;
        info!("Deleted {rows} rows of a user");
        Ok(DeleteUser::NoContent.into())
    }

    /// Record the XP milestone of a lesson unit that skills-ms has completed.
    ///
    /// Only skills-ms calls this, after its own verified completion. The first
    /// call per user and unit awards the XP to the sub-skill; every later call
    /// returns the original milestone and awards nothing. Never costs hearts
    /// and never awards coins.
    #[oai(
        path = "/_internal/lesson-milestones/:user_id/:unit_id",
        method = "put"
    )]
    async fn record_lesson_milestone(
        &self,
        user_id: Path<Uuid>,
        /// The skills-ms unit ID.
        #[oai(validator(pattern = "^[a-z0-9][a-z0-9-]{0,79}$"))]
        unit_id: Path<String>,
        data: Json<RecordLessonMilestoneRequest>,
        db: Data<&DbTxn>,
        _auth: InternalAuth,
    ) -> RecordLessonMilestone::Response<InternalAuth> {
        let data = data.0;
        if data.xp > self.config.challenges.lesson_milestones.max_xp {
            return RecordLessonMilestone::xp_out_of_range();
        }
        // Unknown or root skills would leave an undeliverable XP component.
        if !self
            .state
            .services
            .skills
            .get_skills()
            .await?
            .contains_key(&data.skill_id)
        {
            return RecordLessonMilestone::skill_not_found();
        }
        match lesson_milestones::record(
            &db,
            user_id.0,
            &unit_id.0,
            &data.skill_id,
            data.xp,
            data.completion,
        )
        .await?
        {
            Recorded::Created(milestone) => RecordLessonMilestone::ok(RecordedLessonMilestone {
                created: true,
                milestone,
            }),
            Recorded::Existing(milestone) => RecordLessonMilestone::ok(RecordedLessonMilestone {
                created: false,
                milestone,
            }),
            Recorded::SubjectErased => RecordLessonMilestone::user_erased(),
        }
    }
}

#[derive(Debug, ApiResponse)]
pub enum DeleteUser {
    /// All data of the user has been deleted.
    #[oai(status = 204)]
    NoContent,
}

response!(ExportUser = {
    /// All data of the user.
    Ok(200) => UserDataExport,
});

response!(GetLearningHistory = {
    /// Requested exercises and lectures with existing participation by this user.
    Ok(200) => LearningHistory,
    /// At most 500 combined subtask IDs and lecture bindings are accepted.
    TooManyEntries(422, error),
});

response!(RecordLessonMilestone = {
    /// The milestone as first recorded; see `created`.
    Ok(200) => RecordedLessonMilestone,
    /// The skill is not a known sub-skill. Nothing was recorded.
    SkillNotFound(404, error),
    /// The XP exceed the configured maximum per lesson unit. Nothing was recorded.
    XpOutOfRange(422, error),
    /// The user's account was erased. Nothing was recorded; do not retry.
    UserErased(410, error),
});
