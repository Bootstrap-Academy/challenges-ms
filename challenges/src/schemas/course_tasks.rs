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
    pub section_id: Option<String>,
    /// The lecture this task is associated with
    pub lecture_id: Option<String>,
}

#[derive(Debug, Clone, Object)]
pub struct CreateCourseTaskRequest {
    /// The section this task is associated with
    pub section_id: Option<String>,
    /// The lecture this task is associated with
    pub lecture_id: Option<String>,
}

#[derive(Debug, Clone, Object)]
pub struct UpdateCourseTaskRequest {
    /// The course this task is associated with
    pub course_id: PatchValue<String>,
    /// The section this task is associated with
    pub section_id: PatchValue<Option<String>>,
    /// The lecture this task is associated with
    pub lecture_id: PatchValue<Option<String>>,
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
        }
    }
}
