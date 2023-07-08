use chrono::{DateTime, Utc};
use entity::{challenges_challenge_categories, challenges_challenges, challenges_tasks};
use poem_ext::patch_value::PatchValue;
use poem_openapi::Object;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Object, Deserialize)]
pub struct Category {
    /// The unique identifier of the category
    pub id: Uuid,
    /// The title of the category
    pub title: String,
    /// The description of the category
    pub description: String,
}

#[derive(Debug, Clone, Object, Serialize)]
pub struct CreateCategoryRequest {
    /// The title of the category
    #[oai(validator(max_length = 256))]
    pub title: String,
    /// The description of the category
    #[oai(validator(max_length = 4096))]
    pub description: String,
}

#[derive(Debug, Clone, Object)]
pub struct UpdateCategoryRequest {
    /// The title of the category
    #[oai(validator(max_length = 256))]
    pub title: PatchValue<String>,
    /// The description of the category
    #[oai(validator(max_length = 4096))]
    pub description: PatchValue<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "error", content = "details", rename_all = "snake_case")]
pub enum DeleteCategoryError {
    NotFound,
}

#[derive(Debug, Clone, Object)]
pub struct Challenge {
    /// The unique identifier of the challenge
    pub id: Uuid,
    /// The category of the challenge
    pub category: Uuid,
    /// The title of the challenge
    pub title: String,
    /// The description of the challenge
    pub description: String,
    /// The creator of the challenge
    pub creator: Uuid,
    /// The creation timestamp of the challenge
    pub creation_timestamp: DateTime<Utc>,
    /// The skills of the challenge
    pub skills: Vec<String>,
}

#[derive(Debug, Clone, Object)]
pub struct CreateChallengeRequest {
    /// The title of the challenge
    #[oai(validator(max_length = 256))]
    pub title: String,
    /// The description of the challenge
    #[oai(validator(max_length = 4096))]
    pub description: String,
    /// The skills of the challenge
    #[oai(validator(max_items = 8, unique_items = true))]
    pub skills: Vec<String>,
}

#[derive(Debug, Clone, Object)]
pub struct UpdateChallengeRequest {
    /// The category of the challenge
    pub category: PatchValue<Uuid>,
    /// The title of the challenge
    #[oai(validator(max_length = 256))]
    pub title: PatchValue<String>,
    /// The description of the challenge
    #[oai(validator(max_length = 4096))]
    pub description: PatchValue<String>,
    /// The skills of the challenge
    #[oai(validator(max_items = 8, unique_items = true))]
    pub skills: PatchValue<Vec<String>>,
}

impl From<challenges_challenge_categories::Model> for Category {
    fn from(value: challenges_challenge_categories::Model) -> Self {
        Self {
            id: value.id,
            title: value.title,
            description: value.description,
        }
    }
}

impl Challenge {
    pub fn from(challenge: challenges_challenges::Model, task: challenges_tasks::Model) -> Self {
        Self {
            id: task.id,
            category: challenge.category_id,
            title: challenge.title,
            description: challenge.description,
            creator: task.creator,
            creation_timestamp: task.creation_timestamp.and_utc(),
            skills: challenge.skill_ids,
        }
    }
}
