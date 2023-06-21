use chrono::{DateTime, Utc};
use entity::{challenges_matchings, challenges_subtasks};
use poem_ext::patch_value::PatchValue;
use poem_openapi::Object;
use uuid::Uuid;

macro_rules! matching {
    ($($name:ident(question: $q_ty:ty = $q_val:expr $(, answer: $ans_ty:ty = $ans_val:expr)?);)*) => {
        $(
            #[derive(Debug, Clone, Object)]
            pub struct $name {
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
                /// The entries on the left.
                pub left: $q_ty,
                /// The entries on the right.
                pub right: $q_ty,
                $(
                    /// For each entry on the left the index of its match on the right.
                    pub solution: $ans_ty,
                )?
            }

            impl $name {
                pub fn from(
                    matching: challenges_matchings::Model,
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
                        left: $q_val(matching.left, unlocked),
                        right: $q_val(matching.right, unlocked),
                        $(solution: $ans_val(matching.solution),)?
                    }
                }
            }
        )*
    };
}

matching! {
    MatchingSummary(question: Option<Vec<String>> = |q, u: bool| u.then_some(q));
    Matching(question: Vec<String> = |q, _| q);
    MatchingWithSolution(question: Vec<String> = |q, _| q, answer: Vec<u8> = |a: Vec<i16>| a.into_iter().map(|x| x as _).collect());
}

#[derive(Debug, Clone, Object)]
pub struct CreateMatchingRequest {
    /// The number of xp a user gets for completing this subtask.
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub xp: u64,
    /// The number of morphcoins a user gets for completing this subtask.
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub coins: u64,
    /// The number of morphcoins a user has to pay to access this subtask.
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub fee: u64,
    /// The entries on the left.
    #[oai(validator(min_items = 1, max_items = 32, max_length = 256))]
    pub left: Vec<String>,
    /// The entries on the right.
    #[oai(validator(min_items = 1, max_items = 32, max_length = 256))]
    pub right: Vec<String>,
    /// For each entry on the left the index of its match on the right.
    /// E.g. left=[A, B, C], right=[X, Y, Z], solution=[2, 0, 1] -> AZ, BX, CY
    #[oai(validator(min_items = 1, max_items = 32, maximum(value = "31")))]
    pub solution: Vec<u8>,
}

#[derive(Debug, Clone, Object)]
pub struct UpdateMatchingRequest {
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
    /// The entries on the left.
    #[oai(validator(min_items = 1, max_items = 32, max_length = 256))]
    pub left: PatchValue<Vec<String>>,
    /// The entries on the right.
    #[oai(validator(min_items = 1, max_items = 32, max_length = 256))]
    pub right: PatchValue<Vec<String>>,
    /// For each entry on the left the index of its match on the right.
    /// E.g. left=[A, B, C], right=[X, Y, Z], solution=[2, 0, 1] -> AZ, BX, CY
    #[oai(validator(min_items = 1, max_items = 32, maximum(value = "31")))]
    pub solution: PatchValue<Vec<u8>>,
}

#[derive(Debug, Clone, Object)]
pub struct SolveMatchingRequest {
    /// For each entry on the left the index of its match on the right.
    /// E.g. left=[A, B, C], right=[X, Y, Z], answer=[2, 0, 1] -> AZ, BX, CY
    pub answer: Vec<u8>,
}

#[derive(Debug, Clone, Object)]
pub struct SolveMatchingFeedback {
    /// Whether the user has successfully solved the question.
    pub solved: bool,
    /// The number of correct matches.
    pub correct: usize,
}
