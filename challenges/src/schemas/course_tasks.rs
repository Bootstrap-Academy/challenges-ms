use chrono::{DateTime, Utc};
use entity::{challenges_course_tasks, challenges_tasks};
use poem_ext::patch_value::PatchValue;
use poem_openapi::Object;
use uuid::Uuid;

#[derive(Debug, Clone, Object)]
pub struct CourseTask {
    /// The unique identifier of the task
    pub id: Uuid,
    /// The course this task is associated with
    pub course_id: String,
    /// The section this task is associated with
    pub section_id: String,
    /// The lecture this task is associated with
    pub lecture_id: String,
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
pub struct CreateCourseTaskRequest {
    /// The section this task is associated with
    pub section_id: String,
    /// The lecture this task is associated with
    pub lecture_id: String,
    /// The title of the task
    #[oai(validator(max_length = 256))]
    pub title: String,
    /// The description of the task
    #[oai(validator(max_length = 4096))]
    pub description: String,
}

#[derive(Debug, Clone, Object)]
pub struct UpdateCourseTaskRequest {
    /// The course this task is associated with
    pub course_id: PatchValue<String>,
    /// The section this task is associated with
    pub section_id: PatchValue<String>,
    /// The lecture this task is associated with
    pub lecture_id: PatchValue<String>,
    /// The title of the task
    #[oai(validator(max_length = 256))]
    pub title: PatchValue<String>,
    /// The description of the task
    #[oai(validator(max_length = 4096))]
    pub description: PatchValue<String>,
}

impl CourseTask {
    pub fn from(
        course_task: challenges_course_tasks::Model,
        task: challenges_tasks::Model,
    ) -> Self {
        Self {
            id: task.id,
            course_id: course_task.course_id,
            section_id: course_task.section_id,
            lecture_id: course_task.lecture_id,
            title: task.title,
            description: task.description,
            creator: task.creator,
            creation_timestamp: task.creation_timestamp.and_local_timezone(Utc).unwrap(),
        }
    }
}
