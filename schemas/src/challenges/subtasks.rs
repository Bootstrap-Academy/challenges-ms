use chrono::{DateTime, Utc};
use entity::{
    challenges_ban, challenges_subtask_reports, challenges_subtasks,
    sea_orm_active_enums::{
        ChallengesBanAction, ChallengesRating, ChallengesReportReason, ChallengesSubtaskType,
    },
};
use poem_ext::patch_value::PatchValue;
use poem_openapi::{Enum, Object};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, Object)]
pub struct Subtask {
    /// The unique identifier of the subtask.
    pub id: Uuid,
    /// The parent task.
    pub task_id: Uuid,
    /// The type of the subtask.
    #[oai(rename = "type")]
    pub ty: ChallengesSubtaskType,
    /// The creator of the subtask
    pub creator: Uuid,
    /// The creation timestamp of the subtask
    pub creation_timestamp: DateTime<Utc>,
    /// The number of xp a user gets for completing this subtask.
    pub xp: u64,
    /// The number of morphcoins a user gets for completing this subtask.
    pub coins: u64,
    /// Whether the user has completed this subtask.
    pub solved: bool,
    /// Whether the user has submitted feedback or reported this subtask.
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
    /// Whether the subtask is enabled and visible to normal users.
    pub enabled: PatchValue<bool>,
}

#[derive(Debug, Clone, Object)]
pub struct SubtaskStats {
    /// Total number of subtasks.
    ///
    /// `total` == `solved` + `attempted` + `unattempted`
    pub total: u64,

    /// Number of subtasks the user has already solved.
    pub solved: u64,
    /// Number of subtasks the user has unsuccessfully tried to solve.
    pub attempted: u64,
    /// Number of subtasks the user has not yet tried to solve.
    pub unattempted: u64,
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
    pub subtask_type: ChallengesSubtaskType,
    pub user_id: Option<Uuid>,
    pub timestamp: DateTime<Utc>,
    pub reason: ChallengesReportReason,
    pub comment: String,
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

#[derive(Debug, Clone, Object, Deserialize)]
pub struct SubtasksUserConfig {
    /// The minimum level a normal user needs to have in each skill related to a
    /// task to be able to create subtasks in it.
    pub min_level: u32,
    /// The maximum `xp` value for subtasks created by normal users
    pub max_xp: u64,
    /// The maximum `coins` value for subtasks created by normal users
    pub max_coins: u64,
}

#[derive(Debug, Clone, Object)]
pub struct Ban {
    /// The unique identifier of the ban.
    pub id: Uuid,
    /// The unique identifier of the user who is banned.
    pub user_id: Uuid,
    /// The unique identifier of the admin who banned the user.
    pub creator: Uuid,
    /// The start timestamp of the ban.
    pub start: DateTime<Utc>,
    /// The end timestamp of the ban. Null if this is a permanent ban.
    pub end: Option<DateTime<Utc>>,
    /// Whether the ban is currently active.
    pub active: bool,
    /// The action the user is not allowed to perform due to this ban.
    pub action: ChallengesBanAction,
    /// The reason why the user has been banned.
    pub reason: String,
}

#[derive(Debug, Clone, Object)]
pub struct CreateBanRequest {
    /// The unique identifier of the user who is banned.
    pub user_id: Uuid,
    /// The start timestamp of the ban. Defaults to the current timestamp.
    pub start: Option<DateTime<Utc>>,
    /// The end timestamp of the ban. Null if this is a permanent ban.
    pub end: Option<DateTime<Utc>>,
    /// The action the user is not allowed to perform due to this ban.
    pub action: ChallengesBanAction,
    /// The reason why the user has been banned.
    #[oai(validator(max_length = 4096))]
    pub reason: String,
}

#[derive(Debug, Clone, Object)]
pub struct UpdateBanRequest {
    /// The start timestamp of the ban.
    pub start: PatchValue<DateTime<Utc>>,
    /// The end timestamp of the ban.
    pub end: PatchValue<Option<DateTime<Utc>>>,
    /// Set to `true` to make this ban permanent, overriding `end`.
    #[oai(default)]
    pub permanent: bool,
    /// The action the user is not allowed to perform due to this ban.
    pub action: PatchValue<ChallengesBanAction>,
    /// The reason why the user has been banned.
    #[oai(validator(max_length = 4096))]
    pub reason: PatchValue<String>,
}

impl Report {
    pub fn from(
        report: challenges_subtask_reports::Model,
        subtask: &challenges_subtasks::Model,
    ) -> Self {
        Self {
            id: report.id,
            task_id: subtask.task_id,
            subtask_id: report.subtask_id,
            subtask_type: subtask.ty,
            user_id: report.user_id,
            timestamp: report.timestamp.and_utc(),
            reason: report.reason,
            comment: report.comment,
        }
    }
}

impl Subtask {
    pub fn from(subtask: challenges_subtasks::Model, solved: bool, rated: bool) -> Self {
        Self {
            id: subtask.id,
            task_id: subtask.task_id,
            ty: subtask.ty,
            creator: subtask.creator,
            creation_timestamp: subtask.creation_timestamp.and_utc(),
            xp: subtask.xp as _,
            coins: subtask.coins as _,
            solved,
            rated,
            enabled: subtask.enabled,
        }
    }
}

impl From<challenges_ban::Model> for Ban {
    fn from(value: challenges_ban::Model) -> Self {
        let now = Utc::now().naive_utc();
        Self {
            id: value.id,
            user_id: value.user_id,
            creator: value.creator,
            start: value.start.and_utc(),
            end: value.end.map(|ts| ts.and_utc()),
            active: value.start <= now
                && (value.end.is_none() || value.end.is_some_and(|end| now < end)),
            action: value.action,
            reason: value.reason,
        }
    }
}
