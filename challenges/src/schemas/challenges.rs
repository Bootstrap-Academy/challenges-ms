use chrono::{DateTime, Utc};
use entity::{challenges_challenge_categories, challenges_challenges, challenges_tasks};
use poem_ext::patch_value::PatchValue;
use poem_openapi::Object;
use uuid::Uuid;

#[derive(Debug, Clone, Object)]
pub struct Category {
    /// The unique identifier of the category
    pub id: Uuid,
    /// The title of the category
    pub title: String,
    /// The description of the category
    pub description: String,
}

#[derive(Debug, Clone, Object)]
pub struct CreateCategoryRequest {
    /// The title of the category
    #[oai(validator(max_length = 255))]
    pub title: String,
    /// The description of the category
    #[oai(validator(max_length = 255))]
    pub description: String,
}

#[derive(Debug, Clone, Object)]
pub struct UpdateCategoryRequest {
    /// The title of the category
    #[oai(validator(max_length = 255))]
    pub title: PatchValue<String>,
    /// The description of the category
    #[oai(validator(max_length = 255))]
    pub description: PatchValue<String>,
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
}

#[derive(Debug, Clone, Object)]
pub struct CreateChallengeRequest {
    /// The title of the challenge
    pub title: String,
    /// The description of the challenge
    pub description: String,
}

#[derive(Debug, Clone, Object)]
pub struct UpdateChallengeRequest {
    /// The category of the challenge
    pub category: PatchValue<Uuid>,
    /// The title of the challenge
    pub title: PatchValue<String>,
    /// The description of the challenge
    pub description: PatchValue<String>,
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
            title: task.title,
            description: task.description,
            creator: task.creator,
            creation_timestamp: task.creation_timestamp.and_local_timezone(Utc).unwrap(),
        }
    }
}
