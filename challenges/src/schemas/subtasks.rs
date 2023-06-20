use chrono::{DateTime, Utc};
use entity::{
    challenges_subtask_reports,
    sea_orm_active_enums::{ChallengesRating, ChallengesReportReason},
};
use poem_ext::patch_value::PatchValue;
use poem_openapi::Object;
use uuid::Uuid;

#[derive(Debug, Clone, Object)]
pub struct UpdateSubtaskRequest {
    /// The number of morphcoins a user has to pay to access this subtask.
    pub fee: PatchValue<u64>,
}

#[derive(Debug, Clone, Object)]
pub struct PostFeedbackRequest {
    pub rating: ChallengesRating,
}

#[derive(Debug, Clone, Object)]
pub struct Report {
    pub id: Uuid,
    pub task_id: Uuid,
    pub subtask_id: Uuid,
    pub user_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub reason: ChallengesReportReason,
    pub comment: String,
    pub completed: bool,
    pub completed_by: Option<Uuid>,
    pub completed_timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Object)]
pub struct CreateReportRequest {
    pub task_id: Uuid,
    pub subtask_id: Uuid,
    pub reason: ChallengesReportReason,
    #[oai(validator(max_length = 4096))]
    pub comment: String,
}

impl Report {
    pub fn from(report: challenges_subtask_reports::Model, task_id: Uuid) -> Self {
        Self {
            id: report.id,
            task_id,
            subtask_id: report.subtask_id,
            user_id: report.user_id,
            timestamp: report.timestamp.and_local_timezone(Utc).unwrap(),
            reason: report.reason,
            comment: report.comment,
            completed: report.completed_by.is_some(),
            completed_by: report.completed_by,
            completed_timestamp: report
                .completed_timestamp
                .map(|x| x.and_local_timezone(Utc).unwrap()),
        }
    }
}
