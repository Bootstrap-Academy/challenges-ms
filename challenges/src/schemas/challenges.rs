use entity::challenges_challenge_categories;
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

impl From<challenges_challenge_categories::Model> for Category {
    fn from(value: challenges_challenge_categories::Model) -> Self {
        Self {
            id: value.id,
            title: value.title,
            description: value.description,
        }
    }
}
