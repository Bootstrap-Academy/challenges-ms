use chrono::{DateTime, Utc};
use entity::{challenges_subtasks, sea_orm_active_enums::ChallengesSubtaskType};
use lib::auth::VerifiedUserAuth;
use poem::web::Data;
use poem_ext::{db::DbTxn, response};
use poem_openapi::{param::Path, Object, OpenApi};
use sea_orm::{ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, QueryFilter, Statement};
use uuid::Uuid;

pub struct Attempts {
    pub state: std::sync::Arc<lib::SharedState>,
}

#[derive(Debug, Object)]
struct AttemptEvidence {
    id: Uuid,
    task_id: Uuid,
    subtask_id: Uuid,
    user_id: Uuid,
    solved: bool,
    created_at: DateTime<Utc>,
    hearts_pending: bool,
}

#[OpenApi(tag = "super::Tags::Subtasks")]
impl Attempts {
    /// Read only the current user's exact attempt, including repeat exercises.
    #[oai(
        path = "/tasks/:task_id/:kind/:subtask_id/attempts/:attempt_id",
        method = "get"
    )]
    async fn get_attempt(
        &self,
        task_id: Path<Uuid>,
        kind: Path<String>,
        subtask_id: Path<Uuid>,
        attempt_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetAttempt::Response<VerifiedUserAuth> {
        let (table, column, ty) = match kind.0.as_str() {
            "multiple_choice" => (
                "challenges_multiple_choice_attempts",
                "question_id",
                ChallengesSubtaskType::MultipleChoiceQuestion,
            ),
            "matchings" => (
                "challenges_matching_attempts",
                "matching_id",
                ChallengesSubtaskType::Matching,
            ),
            "questions" => (
                "challenges_question_attempts",
                "question_id",
                ChallengesSubtaskType::Question,
            ),
            _ => return GetAttempt::attempt_not_found(),
        };
        let Some(subtask) = challenges_subtasks::Entity::find_by_id(subtask_id.0)
            .filter(challenges_subtasks::Column::TaskId.eq(task_id.0))
            .one(&***db)
            .await?
        else {
            return GetAttempt::attempt_not_found();
        };
        if subtask.ty != ty
            || (!auth.0.admin
                && (subtask.moderation_removed
                    || (subtask.creator != auth.0.id && !subtask.enabled)))
        {
            return GetAttempt::attempt_not_found();
        }
        // Table and foreign-key column come exclusively from the closed map above.
        let Some(row) = db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
            format!("SELECT a.id,a.solved,a.timestamp,EXISTS(SELECT 1 FROM challenge_heart_operations h WHERE h.id=a.id AND h.state<>'settled') AS hearts_pending FROM {table} a WHERE a.id=$1 AND a.user_id=$2 AND a.{column}=$3"),
            [attempt_id.0.into(), auth.0.id.into(), subtask.id.into()])).await? else { return GetAttempt::attempt_not_found(); };
        let mut pending: bool = row.try_get("", "hearts_pending")?;
        if pending
            && matches!(
                crate::services::hearts::settle(&self.state.db, &self.state.services, attempt_id.0)
                    .await,
                Ok(true)
            )
        {
            pending = false;
        }
        GetAttempt::ok(AttemptEvidence {
            id: row.try_get("", "id")?,
            task_id: task_id.0,
            subtask_id: subtask.id,
            user_id: auth.0.id,
            solved: row.try_get("", "solved")?,
            created_at: row
                .try_get::<chrono::NaiveDateTime>("", "timestamp")?
                .and_utc(),
            hearts_pending: pending,
        })
    }
}

response!(GetAttempt = {
    Ok(200) => AttemptEvidence,
    /// No accessible own attempt exists at this exact path.
    AttemptNotFound(404, error),
});
