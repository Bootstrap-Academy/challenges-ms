use std::sync::Arc;

use chrono::Utc;
use entity::{challenges_skill_tasks, challenges_tasks};
use lib::{
    auth::{AdminAuth, VerifiedUserAuth},
    SharedState,
};
use poem_ext::{patch_value::PatchValue, response, responses::internal_server_error};
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

use crate::schemas::skill_tasks::{CreateSkillTaskRequest, SkillTask, UpdateSkillTaskRequest};

use super::Tags;

pub struct SkillTasks {
    pub state: Arc<SharedState>,
}

#[OpenApi(tag = "Tags::SkillTasks")]
impl SkillTasks {
    /// List all tasks in a skill.
    #[oai(path = "/skills/:skill_id/tasks", method = "get")]
    async fn list_skill_tasks(
        &self,
        skill_id: Path<String>,
        /// Filter by task title
        title: Query<Option<String>>,
        _auth: VerifiedUserAuth,
    ) -> ListSkillTasks::Response<VerifiedUserAuth> {
        let mut query = challenges_skill_tasks::Entity::find()
            .find_also_related(challenges_tasks::Entity)
            .filter(challenges_skill_tasks::Column::SkillId.eq(skill_id.0))
            .order_by_asc(challenges_tasks::Column::Title);
        if let Some(title) = title.0 {
            query = query.filter(challenges_tasks::Column::Title.contains(&title));
        }
        ListSkillTasks::ok(
            query
                .all(&self.state.db)
                .await
                .map_err(internal_server_error)?
                .into_iter()
                .filter_map(|(challenge, task)| Some(SkillTask::from(challenge, task?)))
                .collect(),
        )
    }

    /// Get a skill task by id.
    #[oai(path = "/skills/:skill_id/tasks/:task_id", method = "get")]
    async fn get_skill_task(
        &self,
        skill_id: Path<String>,
        task_id: Path<Uuid>,
        _auth: VerifiedUserAuth,
    ) -> GetSkillTask::Response<VerifiedUserAuth> {
        match get_skill_task(&self.state.db, skill_id.0, task_id.0).await? {
            Some((skill, task)) => GetSkillTask::ok(SkillTask::from(skill, task)),
            None => GetSkillTask::skill_task_not_found(),
        }
    }

    /// Create a new skill task.
    #[oai(path = "/skills/:skill_id/tasks", method = "post")]
    async fn create_skill_task(
        &self,
        skill_id: Path<String>,
        data: Json<CreateSkillTaskRequest>,
        auth: AdminAuth,
    ) -> CreateSkillTask::Response<AdminAuth> {
        let skills = self
            .state
            .services
            .skills
            .get_skills()
            .await
            .map_err(internal_server_error)?;
        if !skills.contains_key(&skill_id.0) {
            return CreateSkillTask::skill_not_found();
        }

        let task = challenges_tasks::ActiveModel {
            id: Set(Uuid::new_v4()),
            title: Set(data.0.title),
            description: Set(data.0.description),
            creator: Set(auth.0.id.parse().map_err(internal_server_error)?),
            creation_timestamp: Set(Utc::now().naive_utc()),
        }
        .insert(&self.state.db)
        .await
        .map_err(internal_server_error)?;

        let skill_task = challenges_skill_tasks::ActiveModel {
            task_id: Set(task.id),
            skill_id: Set(skill_id.0),
        }
        .insert(&self.state.db)
        .await
        .map_err(internal_server_error)?;

        CreateSkillTask::ok(SkillTask::from(skill_task, task))
    }

    /// Update a skill task.
    #[oai(path = "/skills/:skill_id/tasks/:task_id", method = "patch")]
    async fn update_skill_task(
        &self,
        skill_id: Path<String>,
        task_id: Path<Uuid>,
        data: Json<UpdateSkillTaskRequest>,
        _auth: AdminAuth,
    ) -> UpdateSkillTask::Response<AdminAuth> {
        match get_skill_task(&self.state.db, skill_id.0, task_id.0).await? {
            Some((skill_task, task)) => {
                if let PatchValue::Set(skill) = &data.0.skill {
                    let skills = self
                        .state
                        .services
                        .skills
                        .get_skills()
                        .await
                        .map_err(internal_server_error)?;
                    if !skills.contains_key(skill) {
                        return UpdateSkillTask::skill_not_found();
                    }
                }
                let skill_task = challenges_skill_tasks::ActiveModel {
                    task_id: Unchanged(skill_task.task_id),
                    skill_id: data.0.skill.update(skill_task.skill_id),
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
                UpdateSkillTask::ok(SkillTask::from(skill_task, task))
            }
            None => UpdateSkillTask::skill_task_not_found(),
        }
    }

    /// Delete a skill task.
    #[oai(path = "/skills/:skill_id/tasks/:task_id", method = "delete")]
    async fn delete_skill_task(
        &self,
        skill_id: Path<String>,
        task_id: Path<Uuid>,
        _auth: AdminAuth,
    ) -> DeleteSkillTask::Response<AdminAuth> {
        match get_skill_task(&self.state.db, skill_id.0, task_id.0).await? {
            Some((_, task)) => {
                task.delete(&self.state.db)
                    .await
                    .map_err(internal_server_error)?;
                DeleteSkillTask::ok()
            }
            None => DeleteSkillTask::skill_task_not_found(),
        }
    }
}

response!(ListSkillTasks = {
    Ok(200) => Vec<SkillTask>,
});

response!(GetSkillTask = {
    Ok(200) => SkillTask,
    /// Skill task does not exist.
    SkillTaskNotFound(404, error),
});

response!(CreateSkillTask = {
    Ok(201) => SkillTask,
    /// Skill does not exist.
    SkillNotFound(404, error),
});

response!(UpdateSkillTask = {
    Ok(200) => SkillTask,
    /// Skill task does not exist.
    SkillTaskNotFound(404, error),
    /// Skill does not exist.
    SkillNotFound(404, error),
});

response!(DeleteSkillTask = {
    Ok(200),
    /// Skill task does not exist.
    SkillTaskNotFound(404, error),
});

async fn get_skill_task(
    db: &DatabaseConnection,
    skill_id: String,
    task_id: Uuid,
) -> poem::Result<Option<(challenges_skill_tasks::Model, challenges_tasks::Model)>> {
    Ok(
        match challenges_skill_tasks::Entity::find_by_id(task_id)
            .find_also_related(challenges_tasks::Entity)
            .filter(challenges_skill_tasks::Column::SkillId.eq(skill_id))
            .one(db)
            .await
            .map_err(internal_server_error)?
        {
            Some((skill_task, Some(task))) => Some((skill_task, task)),
            _ => None,
        },
    )
}
