use entity::{
    challenges_challenges, challenges_course_tasks, challenges_subtasks, challenges_tasks,
};
use lib::services::{
    shop::AddCoinsError, skills::AddSkillProgressError, ServiceError, ServiceResult, Services,
};
use sea_orm::{DatabaseTransaction, DbErr, ModelTrait};
use thiserror::Error;
use uuid::Uuid;

use super::course_tasks::get_skills_of_course;

pub async fn send_task_rewards(
    services: &Services,
    db: &DatabaseTransaction,
    user_id: Uuid,
    subtask: &challenges_subtasks::Model,
) -> Result<(), SendTaskRewardsError> {
    let skills = get_skill(
        services,
        get_parent_task(db, subtask)
            .await?
            .ok_or(SendTaskRewardsError::NoParentTask)?
            .1,
    )
    .await?;
    services
        .shop
        .add_coins(user_id, subtask.coins, "Challenges / Aufgaben", true)
        .await??;
    for skill in &skills {
        services
            .skills
            .add_skill_progress(user_id, skill, subtask.xp / skills.len() as i64)
            .await??;
    }
    Ok(())
}

pub async fn get_parent_task(
    db: &DatabaseTransaction,
    subtask: &challenges_subtasks::Model,
) -> Result<Option<(challenges_tasks::Model, Task)>, DbErr> {
    let Some(task) = subtask
        .find_related(challenges_tasks::Entity)
        .one(db)
        .await? else {return Ok(None)};
    if let Some(challenge) = task
        .find_related(challenges_challenges::Entity)
        .one(db)
        .await?
    {
        return Ok(Some((task, Task::Challenge(challenge))));
    }
    if let Some(course_task) = task
        .find_related(challenges_course_tasks::Entity)
        .one(db)
        .await?
    {
        return Ok(Some((task, Task::CourseTask(course_task))));
    }
    Ok(None)
}

pub async fn get_skill(services: &Services, task: Task) -> ServiceResult<Vec<String>> {
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

#[derive(Debug)]
pub enum Task {
    Challenge(challenges_challenges::Model),
    CourseTask(challenges_course_tasks::Model),
}
