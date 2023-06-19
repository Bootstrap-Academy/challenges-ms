use std::collections::HashSet;

use entity::{challenges_subtasks, challenges_tasks, challenges_user_subtasks};
use lib::{
    auth::User,
    config::Config,
    services::{
        shop::AddCoinsError, skills::AddSkillProgressError, ServiceError, ServiceResult, Services,
    },
};
use sea_orm::{ColumnTrait, DatabaseTransaction, DbErr, EntityTrait, ModelTrait, QueryFilter};
use thiserror::Error;
use uuid::Uuid;

use super::{
    course_tasks::get_skills_of_course,
    tasks::{get_specific_task, Task},
};

pub async fn send_task_rewards(
    services: &Services,
    db: &DatabaseTransaction,
    user_id: Uuid,
    subtask: &challenges_subtasks::Model,
) -> Result<(), SendTaskRewardsError> {
    if subtask.xp != 0 {
        let skills = get_skills(
            services,
            get_parent_task(db, subtask)
                .await?
                .ok_or(SendTaskRewardsError::NoParentTask)?
                .1,
        )
        .await?;
        for skill in &skills {
            services
                .skills
                .add_skill_progress(user_id, skill, subtask.xp / skills.len() as i64)
                .await??;
        }
    }
    if subtask.coins != 0 {
        services
            .shop
            .add_coins(user_id, subtask.coins, "Challenges / Aufgaben", true)
            .await??;
    }
    Ok(())
}

pub async fn get_unlocked(db: &DatabaseTransaction, user_id: Uuid) -> Result<HashSet<Uuid>, DbErr> {
    Ok(challenges_user_subtasks::Entity::find()
        .filter(challenges_user_subtasks::Column::UserId.eq(user_id))
        .all(db)
        .await?
        .into_iter()
        .filter(|x| x.unlocked_timestamp.is_some())
        .map(|x| x.subtask_id)
        .collect())
}

pub async fn check_unlocked(
    db: &DatabaseTransaction,
    user: &User,
    subtask: &challenges_subtasks::Model,
) -> Result<bool, DbErr> {
    Ok(user.admin
        || user.id == subtask.creator
        || subtask.fee == 0
        || challenges_user_subtasks::Entity::find()
            .filter(challenges_user_subtasks::Column::UserId.eq(user.id))
            .filter(challenges_user_subtasks::Column::SubtaskId.eq(subtask.id))
            .filter(challenges_user_subtasks::Column::UnlockedTimestamp.is_not_null())
            .one(db)
            .await?
            .is_some())
}

pub async fn can_create(
    services: &Services,
    config: &Config,
    task: &Task,
    user: &User,
) -> Result<bool, CheckPermissionsError> {
    Ok(match task {
        Task::Challenge(_) => user.admin,
        Task::CourseTask(t) => can_create_for_course(services, config, &t.course_id, user).await?,
    })
}

pub async fn can_create_for_course(
    services: &Services,
    config: &Config,
    course_id: &str,
    user: &User,
) -> Result<bool, CheckPermissionsError> {
    if user.admin {
        return Ok(true);
    }

    let skills = get_skills_of_course(services, course_id).await?;
    let levels = services.skills.get_skill_levels(user.id).await?;
    Ok(skills.iter().all(|skill| {
        levels
            .get(skill)
            .is_some_and(|&level| level >= config.challenges.quizzes.min_level)
    }))
}

pub async fn get_parent_task(
    db: &DatabaseTransaction,
    subtask: &challenges_subtasks::Model,
) -> Result<Option<(challenges_tasks::Model, Task)>, DbErr> {
    Ok(
        match subtask
            .find_related(challenges_tasks::Entity)
            .one(db)
            .await?
        {
            Some(task) => get_specific_task(db, &task).await?.map(|x| (task, x)),
            None => None,
        },
    )
}

pub async fn get_skills(services: &Services, task: Task) -> ServiceResult<Vec<String>> {
    Ok(match task {
        Task::Challenge(challenge) => challenge.skill_ids,
        Task::CourseTask(task) => get_skills_of_course(services, &task.course_id).await?,
    })
}

#[derive(Debug, Error)]
pub enum SendTaskRewardsError {
    #[error("service error: {0}")]
    ServiceError(#[from] ServiceError),
    #[error("database error: {0}")]
    DbErr(#[from] DbErr),
    #[error("could not find parent task")]
    NoParentTask,
    #[error("could not add coins: {0}")]
    AddCoins(#[from] AddCoinsError),
    #[error("could not add xp: {0}")]
    AddXp(#[from] AddSkillProgressError),
}

#[derive(Debug, Error)]
pub enum CheckPermissionsError {
    #[error("service error: {0}")]
    ServiceError(#[from] ServiceError),
    #[error("database error: {0}")]
    DbErr(#[from] DbErr),
}
