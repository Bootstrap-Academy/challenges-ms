use chrono::{DateTime, Utc};
use entity::{
    challenges_subtask_reports, challenges_subtasks,
    sea_orm_active_enums::{ChallengesRating, ChallengesReportReason},
};
use poem_ext::patch_value::PatchValue;
use poem_openapi::{Enum, Object};
use uuid::Uuid;

#[derive(Debug, Clone, Object)]
pub struct Subtask {
    /// The unique identifier of the subtask.
    pub id: Uuid,
    /// The parent task.
    pub task_id: Uuid,
    /// The creator of the subtask
    pub creator: Uuid,
    /// The creation timestamp of the subtask
    pub creation_timestamp: DateTime<Utc>,
    /// The number of xp a user gets for completing this subtask.
    pub xp: u64,
    /// The number of morphcoins a user gets for completing this subtask.
    pub coins: u64,
    /// The number of morphcoins a user has to pay to access this subtask.
    pub fee: u64,
    /// Whether the user has unlocked this subtask.
    pub unlocked: bool,
    /// Whether the user has completed this subtask.
    pub solved: bool,
    /// Whether the user has rated this subtask.
    pub rated: bool,
    /// Whether the subtask is enabled and visible to normal users.
    pub enabled: bool,
}

#[derive(Debug, Clone, Object)]
pub struct CreateSubtaskRequest {
    /// The number of xp a user gets for completing this subtask. Omit to use
    /// the configured default value.
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub xp: Option<u64>,
    /// The number of morphcoins a user gets for completing this subtask. Omit
    /// to use the configured default value.
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub coins: Option<u64>,
    /// The number of morphcoins a user has to pay to access this subtask.
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub fee: u64,
}

#[derive(Debug, Clone, Object)]
pub struct UpdateSubtaskRequest {
    /// The parent task.
    pub task_id: PatchValue<Uuid>,
    /// The number of xp a user gets for completing this subtask.
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub xp: PatchValue<u64>,
    /// The number of morphcoins a user gets for completing this subtask.
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub coins: PatchValue<u64>,
    /// The number of morphcoins a user has to pay to access this subtask.
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub fee: PatchValue<u64>,
    /// Whether the subtask is enabled and visible to normal users.
    pub enabled: PatchValue<bool>,
}

#[derive(Debug, Clone, Object)]
pub struct UserUpdateSubtaskRequest {
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
    pub user_id: Option<Uuid>,
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

#[derive(Debug, Clone, Object)]
pub struct ResolveReportRequest {
    pub action: ResolveReportAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
#[oai(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResolveReportAction {
    Revise,
    BlockReporter,
    BlockCreator,
}

#[derive(Debug, Clone, Object)]
pub struct SubtasksUserConfig {
    /// The minimum level a normal user needs to have in each skill related to a
    /// task to be able to create subtasks in it.
    pub min_level: u32,
    /// The maximum `xp` value for subtasks created by normal users
    pub max_xp: u64,
    /// The maximum `coins` value for subtasks created by normal users
    pub max_coins: u64,
    /// The maximum `fee` value for subtasks created by normal users
    pub max_fee: u64,
}

impl Report {
    pub fn from(report: challenges_subtask_reports::Model, task_id: Uuid) -> Self {
        Self {
            id: report.id,
            task_id,
            subtask_id: report.subtask_id,
            user_id: report.user_id,
            timestamp: report.timestamp.and_utc(),
            reason: report.reason,
            comment: report.comment,
            completed: report.completed_by.is_some(),
            completed_by: report.completed_by,
            completed_timestamp: report.completed_timestamp.map(|x| x.and_utc()),
        }
    }
}

impl Subtask {
    pub fn from(
        subtask: challenges_subtasks::Model,
        unlocked: bool,
        solved: bool,
        rated: bool,
    ) -> Self {
        Self {
            id: subtask.id,
            task_id: subtask.task_id,
            creator: subtask.creator,
            creation_timestamp: subtask.creation_timestamp.and_utc(),
            xp: subtask.xp as _,
            coins: subtask.coins as _,
            fee: subtask.fee as _,
            unlocked,
            solved,
            rated,
            enabled: subtask.enabled,
        }
    }
}
