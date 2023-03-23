use chrono::{DateTime, Utc};
use entity::{challenges_skill_tasks, challenges_tasks};
use poem_ext::patch_value::PatchValue;
use poem_openapi::Object;
use uuid::Uuid;

#[derive(Debug, Clone, Object)]
pub struct SkillTask {
    /// The unique identifier of the task
    pub id: Uuid,
    /// The skill of the task
    pub skill: String,
    /// The title of the task
    pub title: String,
    /// The description of the task
    pub description: String,
    /// The creator of the task
    pub creator: Uuid,
    /// The creation timestamp of the task
    pub creation_timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Object)]
pub struct CreateSkillTaskRequest {
    /// The title of the task
    pub title: String,
    /// The description of the task
    pub description: String,
}

#[derive(Debug, Clone, Object)]
pub struct UpdateSkillTaskRequest {
    /// The skill of the task
    pub skill: PatchValue<String>,
    /// The title of the task
    pub title: PatchValue<String>,
    /// The description of the task
    pub description: PatchValue<String>,
}

impl SkillTask {
    pub fn from(skill_task: challenges_skill_tasks::Model, task: challenges_tasks::Model) -> Self {
        Self {
            id: task.id,
            skill: skill_task.skill_id,
            title: task.title,
            description: task.description,
            creator: task.creator,
            creation_timestamp: task.creation_timestamp.and_local_timezone(Utc).unwrap(),
        }
    }
}
