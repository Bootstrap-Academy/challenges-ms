use std::sync::Arc;

use chrono::Utc;
use entity::{challenges_course_tasks, challenges_tasks};
use lib::{auth::VerifiedUserAuth, config::Config, services::Services, SharedState};
use poem::web::Data;
use poem_ext::{db::DbTxn, response, responses::ErrorResponse};
use poem_openapi::{
    param::{Path, Query},
    payload::Json,
    OpenApi,
};
use schemas::challenges::course_tasks::{CourseTask, CreateCourseTaskRequest};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseTransaction, EntityTrait, QueryFilter, Set,
};
use uuid::Uuid;

use super::Tags;
use crate::services::subtasks::can_create_for_course;

pub struct CourseTasks {
    pub state: Arc<SharedState>,
    pub config: Arc<Config>,
}

#[OpenApi(tag = "Tags::CourseTasks")]
impl CourseTasks {
    /// List all tasks in a skill.
    #[oai(path = "/skills/:skill_id/tasks", method = "get")]
    async fn list_tasks_in_skill(
        &self,
        skill_id: Path<String>,
        db: Data<&DbTxn>,
        _auth: VerifiedUserAuth,
    ) -> ListTasksInSkill::Response<VerifiedUserAuth> {
        let skill = match self
            .state
            .services
            .skills
            .get_skills()
            .await?
            .remove(&skill_id.0)
        {
            Some(skill) => skill,
            None => return ListTasksInSkill::not_found(),
        };

        let condition = skill.courses.into_iter().fold(Condition::any(), |acc, e| {
            acc.add(challenges_course_tasks::Column::CourseId.eq(e))
        });

        let query = challenges_course_tasks::Entity::find()
            .find_also_related(challenges_tasks::Entity)
            .filter(condition);
        ListTasksInSkill::ok(
            query
                .all(&***db)
                .await?
                .into_iter()
                .filter_map(|(challenge, task)| Some(CourseTask::from(challenge, task?)))
                .collect(),
        )
    }

    /// List all tasks in a course.
    #[oai(path = "/courses/:course_id/tasks", method = "get")]
    async fn list_course_tasks(
        &self,
        course_id: Path<String>,
        /// Filter by section id
        section_id: Query<Option<String>>,
        /// Filter by lecture id
        lecture_id: Query<Option<String>>,
        db: Data<&DbTxn>,
        _auth: VerifiedUserAuth,
    ) -> ListCourseTasks::Response<VerifiedUserAuth> {
        let mut query = challenges_course_tasks::Entity::find()
            .find_also_related(challenges_tasks::Entity)
            .filter(challenges_course_tasks::Column::CourseId.eq(course_id.0));
        if let Some(section_id) = section_id.0 {
            query = query.filter(challenges_course_tasks::Column::SectionId.eq(section_id));
        }
        if let Some(lecture_id) = lecture_id.0 {
            query = query.filter(challenges_course_tasks::Column::LectureId.eq(lecture_id));
        }
        ListCourseTasks::ok(
            query
                .all(&***db)
                .await?
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
        db: Data<&DbTxn>,
        _auth: VerifiedUserAuth,
    ) -> GetCourseTask::Response<VerifiedUserAuth> {
        match get_course_task(&db, course_id.0, task_id.0).await? {
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
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> CreateCourseTask::Response<VerifiedUserAuth> {
        if data.0.lecture_id.is_some() && data.0.section_id.is_none() {
            return CreateCourseTask::lecture_without_section();
        }

        match check_course(
            &self.state.services,
            &course_id.0,
            data.0.section_id.as_deref(),
            data.0.lecture_id.as_deref(),
        )
        .await?
        {
            Ok(_) => {}
            Err(CourseNotFoundError::Course) => return CreateCourseTask::course_not_found(),
            Err(CourseNotFoundError::Section) => return CreateCourseTask::section_not_found(),
            Err(CourseNotFoundError::Lecture) => return CreateCourseTask::lecture_not_found(),
        }

        if !can_create_for_course(&self.state.services, &self.config, &course_id.0, &auth.0).await?
        {
            return CreateCourseTask::forbidden();
        }

        let eq = |x: challenges_course_tasks::Column, y| match y {
            Some(y) => x.eq(y),
            None => x.is_null(),
        };

        if let Some((course_task, Some(task))) = challenges_course_tasks::Entity::find()
            .find_also_related(challenges_tasks::Entity)
            .filter(challenges_course_tasks::Column::CourseId.eq(&course_id.0))
            .filter(eq(
                challenges_course_tasks::Column::SectionId,
                data.0.section_id.as_deref(),
            ))
            .filter(eq(
                challenges_course_tasks::Column::LectureId,
                data.0.lecture_id.as_deref(),
            ))
            .one(&***db)
            .await?
        {
            return CreateCourseTask::ok(CourseTask::from(course_task, task));
        }

        let task = challenges_tasks::ActiveModel {
            id: Set(Uuid::new_v4()),
            creator: Set(auth.0.id),
            creation_timestamp: Set(Utc::now().naive_utc()),
        }
        .insert(&***db)
        .await?;

        let course_task = challenges_course_tasks::ActiveModel {
            task_id: Set(task.id),
            course_id: Set(course_id.0),
            section_id: Set(data.0.section_id),
            lecture_id: Set(data.0.lecture_id),
        }
        .insert(&***db)
        .await?;

        CreateCourseTask::created(CourseTask::from(course_task, task))
    }
}

response!(ListTasksInSkill = {
    Ok(200) => Vec<CourseTask>,
    /// Skill does not exist.
    NotFound(404, error),
});

response!(ListCourseTasks = {
    Ok(200) => Vec<CourseTask>,
});

response!(GetCourseTask = {
    Ok(200) => CourseTask,
    /// Course task does not exist.
    CourseTaskNotFound(404, error),
});

response!(CreateCourseTask = {
    Created(201) => CourseTask,
    Ok(200) => CourseTask,
    /// Course does not exist.
    CourseNotFound(404, error),
    /// Section does not exist.
    SectionNotFound(404, error),
    /// Lecture does not exist.
    LectureNotFound(404, error),
    /// Cannot set lecture id without section id
    LectureWithoutSection(400, error),
    /// The user is not allowed to create this course task.
    Forbidden(403, error),
});

async fn get_course_task(
    db: &DatabaseTransaction,
    course_id: String,
    task_id: Uuid,
) -> Result<Option<(challenges_course_tasks::Model, challenges_tasks::Model)>, ErrorResponse> {
    Ok(
        match challenges_course_tasks::Entity::find_by_id(task_id)
            .find_also_related(challenges_tasks::Entity)
            .filter(challenges_course_tasks::Column::CourseId.eq(course_id))
            .one(db)
            .await?
        {
            Some((course_task, Some(task))) => Some((course_task, task)),
            _ => None,
        },
    )
}

async fn check_course(
    services: &Services,
    course_id: &str,
    section_id: Option<&str>,
    lecture_id: Option<&str>,
) -> Result<Result<(), CourseNotFoundError>, ErrorResponse> {
    let courses = services.skills.get_courses().await?;
    let Some(course) = courses.get(course_id) else {
        return Ok(Err(CourseNotFoundError::Course));
    };

    let Some(section_id) = section_id else {
        return Ok(Ok(()));
    };
    let Some(section) = course.sections.iter().find(|e| e.id == section_id) else {
        return Ok(Err(CourseNotFoundError::Section));
    };

    let Some(lecture_id) = lecture_id else {
        return Ok(Ok(()));
    };
    let Some(_lecture) = section.lectures.iter().find(|e| e.id == lecture_id) else {
        return Ok(Err(CourseNotFoundError::Lecture));
    };

    Ok(Ok(()))
}

#[derive(Debug)]
enum CourseNotFoundError {
    Course,
    Section,
    Lecture,
}
