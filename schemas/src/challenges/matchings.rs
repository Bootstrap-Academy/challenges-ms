use entity::challenges_matchings;
use poem_ext::patch_value::PatchValue;
use poem_openapi::Object;

use super::subtasks::{CreateSubtaskRequest, Subtask, UpdateSubtaskRequest};

#[derive(Debug, Clone, Object)]
pub struct MatchingSummary {
    #[oai(flatten)]
    pub subtask: Subtask,
    /// The entries on the left.
    pub left: Option<Vec<String>>,
    /// The entries on the right.
    pub right: Option<Vec<String>>,
}

#[derive(Debug, Clone, Object)]
pub struct Matching {
    #[oai(flatten)]
    pub subtask: Subtask,
    /// The entries on the left.
    pub left: Vec<String>,
    /// The entries on the right.
    pub right: Vec<String>,
}

#[derive(Debug, Clone, Object)]
pub struct MatchingWithSolution {
    #[oai(flatten)]
    pub subtask: Subtask,
    /// The entries on the left.
    pub left: Vec<String>,
    /// The entries on the right.
    pub right: Vec<String>,
    /// For each entry on the left the index of its match on the right.
    pub solution: Vec<u8>,
}

#[derive(Debug, Clone, Object)]
pub struct CreateMatchingRequest {
    #[oai(flatten)]
    pub subtask: CreateSubtaskRequest,
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
    #[oai(flatten)]
    pub subtask: UpdateSubtaskRequest,
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

impl MatchingSummary {
    pub fn from(matching: challenges_matchings::Model, subtask: Subtask) -> Self {
        Self {
            left: subtask.unlocked.then_some(matching.left),
            right: subtask.unlocked.then_some(matching.right),
            subtask,
        }
    }
}

impl Matching {
    pub fn from(matching: challenges_matchings::Model, subtask: Subtask) -> Self {
        Self {
            left: matching.left,
            right: matching.right,
            subtask,
        }
    }
}

impl MatchingWithSolution {
    pub fn from(matching: challenges_matchings::Model, subtask: Subtask) -> Self {
        Self {
            left: matching.left,
            right: matching.right,
            solution: matching.solution.into_iter().map(|x| x as _).collect(),
            subtask,
        }
    }
}
