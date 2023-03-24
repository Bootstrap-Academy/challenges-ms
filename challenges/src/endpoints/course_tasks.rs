use std::sync::Arc;

use chrono::Utc;
use entity::{challenges_course_tasks, challenges_tasks};
use lib::{
    auth::{AdminAuth, VerifiedUserAuth},
    services::Services,
    SharedState,
};
use poem_ext::{response, responses::internal_server_error};
use poem_openapi::{
    param::{Path, Query},
    payload::Json,
    OpenApi,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, ModelTrait, QueryFilter,
    QueryOrder, Set, Unchanged,
};
use uuid::Uuid;

use crate::schemas::course_tasks::{CourseTask, CreateCourseTaskRequest, UpdateCourseTaskRequest};

use super::Tags;

pub struct CourseTasks {
    pub state: Arc<SharedState>,
}

#[OpenApi(tag = "Tags::CourseTasks")]
impl CourseTasks {
    /// List all tasks in a course.
    #[oai(path = "/courses/:course_id/tasks", method = "get")]
    async fn list_course_tasks(
        &self,
        course_id: Path<String>,
        /// Filter by task title
        title: Query<Option<String>>,
        /// Filter by section id
        section_id: Query<Option<String>>,
        /// Filter by lecture id
        lecture_id: Query<Option<String>>,
        _auth: VerifiedUserAuth,
    ) -> ListCourseTasks::Response<VerifiedUserAuth> {
        let mut query = challenges_course_tasks::Entity::find()
            .find_also_related(challenges_tasks::Entity)
            .filter(challenges_course_tasks::Column::CourseId.eq(course_id.0))
            .order_by_asc(challenges_tasks::Column::Title);
        if let Some(title) = title.0 {
            query = query.filter(challenges_tasks::Column::Title.contains(&title));
        }
        if let Some(section_id) = section_id.0 {
            query = query.filter(challenges_course_tasks::Column::SectionId.eq(section_id));
        }
        if let Some(lecture_id) = lecture_id.0 {
            query = query.filter(challenges_course_tasks::Column::LectureId.eq(lecture_id));
        }
        ListCourseTasks::ok(
            query
                .all(&self.state.db)
                .await
                .map_err(internal_server_error)?
                .into_iter()
                .filter_map(|(challenge, task)| Some(CourseTask::from(challenge, task?)))
                .collect(),
        )
    }

    /// Get a course task by id.
    #[oai(path = "/courses/:course_id/tasks/:task_id", method = "get")]
    async fn get_course_task(
        &self,
        course_id: Path<String>,
        task_id: Path<Uuid>,
        _auth: VerifiedUserAuth,
    ) -> GetCourseTask::Response<VerifiedUserAuth> {
        match get_course_task(&self.state.db, course_id.0, task_id.0).await? {
            Some((course, task)) => GetCourseTask::ok(CourseTask::from(course, task)),
            None => GetCourseTask::course_task_not_found(),
        }
    }

    /// Create a new course task.
    #[oai(path = "/courses/:course_id/tasks", method = "post")]
    async fn create_course_task(
        &self,
        course_id: Path<String>,
        data: Json<CreateCourseTaskRequest>,
        auth: AdminAuth,
    ) -> CreateCourseTask::Response<AdminAuth> {
        if !check_course(
            &self.state.services,
            &course_id.0,
            &data.0.section_id,
            &data.0.lecture_id,
        )
        .await?
        {
            return CreateCourseTask::course_not_found();
        }

        let task = challenges_tasks::ActiveModel {
            id: Set(Uuid::new_v4()),
            title: Set(data.0.title),
            description: Set(data.0.description),
            creator: Set(auth.0.id),
            creation_timestamp: Set(Utc::now().naive_utc()),
        }
        .insert(&self.state.db)
        .await
        .map_err(internal_server_error)?;

        let course_task = challenges_course_tasks::ActiveModel {
            task_id: Set(task.id),
            course_id: Set(course_id.0),
            section_id: Set(data.0.section_id),
            lecture_id: Set(data.0.lecture_id),
        }
        .insert(&self.state.db)
        .await
        .map_err(internal_server_error)?;

        CreateCourseTask::ok(CourseTask::from(course_task, task))
    }

    /// Update a course task.
    #[oai(path = "/courses/:course_id/tasks/:task_id", method = "patch")]
    async fn update_course_task(
        &self,
        course_id: Path<String>,
        task_id: Path<Uuid>,
        data: Json<UpdateCourseTaskRequest>,
        _auth: AdminAuth,
    ) -> UpdateCourseTask::Response<AdminAuth> {
        match get_course_task(&self.state.db, course_id.0, task_id.0).await? {
            Some((course_task, task)) => {
                let course_id = data.0.course_id.get_new(&course_task.course_id);
                let section_id = &data.0.section_id.get_new(&course_task.section_id);
                let lecture_id = &data.0.lecture_id.get_new(&course_task.lecture_id);
                if !check_course(&self.state.services, course_id, section_id, lecture_id).await? {
                    return UpdateCourseTask::course_not_found();
                }

                let course_task = challenges_course_tasks::ActiveModel {
                    task_id: Unchanged(course_task.task_id),
                    course_id: data.0.course_id.update(course_task.course_id),
                    section_id: data.0.section_id.update(course_task.section_id),
                    lecture_id: data.0.lecture_id.update(course_task.lecture_id),
                }
                .update(&self.state.db)
                .await
                .map_err(internal_server_error)?;
                let task = challenges_tasks::ActiveModel {
                    id: Unchanged(task.id),
                    title: data.0.title.update(task.title),
                    description: data.0.description.update(task.description),
                    creator: Unchanged(task.creator),
                    creation_timestamp: Unchanged(task.creation_timestamp),
                }
                .update(&self.state.db)
                .await
                .map_err(internal_server_error)?;
                UpdateCourseTask::ok(CourseTask::from(course_task, task))
            }
            None => UpdateCourseTask::course_task_not_found(),
        }
    }

    /// Delete a course task.
    #[oai(path = "/courses/:course_id/tasks/:task_id", method = "delete")]
    async fn delete_course_task(
        &self,
        course_id: Path<String>,
        task_id: Path<Uuid>,
        _auth: AdminAuth,
    ) -> DeleteCourseTask::Response<AdminAuth> {
        match get_course_task(&self.state.db, course_id.0, task_id.0).await? {
            Some((_, task)) => {
                task.delete(&self.state.db)
                    .await
                    .map_err(internal_server_error)?;
                DeleteCourseTask::ok()
            }
            None => DeleteCourseTask::course_task_not_found(),
        }
    }
}

response!(ListCourseTasks = {
    Ok(200) => Vec<CourseTask>,
});

response!(GetCourseTask = {
    Ok(200) => CourseTask,
    /// Course task does not exist.
    CourseTaskNotFound(404, error),
});

response!(CreateCourseTask = {
    Ok(201) => CourseTask,
    /// Course does not exist.
    CourseNotFound(404, error),
});

response!(UpdateCourseTask = {
    Ok(200) => CourseTask,
    /// Course task does not exist.
    CourseTaskNotFound(404, error),
    /// Course does not exist.
    CourseNotFound(404, error),
});

response!(DeleteCourseTask = {
    Ok(200),
    /// Course task does not exist.
    CourseTaskNotFound(404, error),
});

async fn get_course_task(
    db: &DatabaseConnection,
    course_id: String,
    task_id: Uuid,
) -> poem::Result<Option<(challenges_course_tasks::Model, challenges_tasks::Model)>> {
    Ok(
        match challenges_course_tasks::Entity::find_by_id(task_id)
            .find_also_related(challenges_tasks::Entity)
            .filter(challenges_course_tasks::Column::CourseId.eq(course_id))
            .one(db)
            .await
            .map_err(internal_server_error)?
        {
            Some((course_task, Some(task))) => Some((course_task, task)),
            _ => None,
        },
    )
}

async fn check_course(
    services: &Services,
    course_id: &str,
    section_id: &str,
    lecture_id: &str,
) -> poem::Result<bool> {
    let courses = services
        .skills
        .get_courses()
        .await
        .map_err(internal_server_error)?;
    Ok((|| {
        courses
            .get(course_id)?
            .sections
            .iter()
            .find(|e| e.id == section_id)?
            .lectures
            .iter()
            .find(|e| e.id == lecture_id)
    })()
    .is_some())
}
