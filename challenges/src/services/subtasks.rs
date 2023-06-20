use std::collections::HashMap;

use chrono::{DateTime, NaiveDateTime, Utc};
use entity::{
    challenges_ban, challenges_subtasks, challenges_tasks, challenges_user_subtasks,
    sea_orm_active_enums::ChallengesBanAction,
};
use lib::{
    auth::User,
    config::Config,
    services::{
        shop::AddCoinsError, skills::AddSkillProgressError, ServiceError, ServiceResult, Services,
    },
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, DbErr, EntityTrait, ModelTrait,
    QueryFilter, Unchanged,
};
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

pub async fn get_user_subtasks(
    db: &DatabaseTransaction,
    user_id: Uuid,
) -> Result<HashMap<Uuid, challenges_user_subtasks::Model>, DbErr> {
    Ok(challenges_user_subtasks::Entity::find()
        .filter(challenges_user_subtasks::Column::UserId.eq(user_id))
        .all(db)
        .await?
        .into_iter()
        .map(|x| (x.subtask_id, x))
        .collect())
}

pub async fn get_user_subtask(
    db: &DatabaseTransaction,
    user_id: Uuid,
    subtask_id: Uuid,
) -> Result<Option<challenges_user_subtasks::Model>, DbErr> {
    challenges_user_subtasks::Entity::find()
        .filter(challenges_user_subtasks::Column::UserId.eq(user_id))
        .filter(challenges_user_subtasks::Column::SubtaskId.eq(subtask_id))
        .one(db)
        .await
}

pub async fn update_user_subtask(
    db: &DatabaseTransaction,
    user_subtask: Option<&challenges_user_subtasks::Model>,
    values: challenges_user_subtasks::ActiveModel,
) -> Result<challenges_user_subtasks::Model, DbErr> {
    if let Some(user_subtask) = user_subtask {
        challenges_user_subtasks::ActiveModel {
            user_id: Unchanged(user_subtask.user_id),
            subtask_id: Unchanged(user_subtask.subtask_id),
            ..values
        }
        .update(db)
        .await
    } else {
        challenges_user_subtasks::ActiveModel { ..values }
            .insert(db)
            .await
    }
}

pub async fn get_active_ban(
    db: &DatabaseTransaction,
    user: &User,
    action: ChallengesBanAction,
) -> Result<ActiveBan, DbErr> {
    if user.admin {
        return Ok(ActiveBan::NotBanned);
    }
    let bans = challenges_ban::Entity::find()
        .filter(challenges_ban::Column::UserId.eq(user.id))
        .filter(challenges_ban::Column::Action.eq(action))
        .all(db)
        .await?;
    let now = Utc::now().naive_utc();
    let active = bans
        .iter()
        .map(|x| x.end.unwrap_or(NaiveDateTime::MAX))
        .filter(|x| *x > now)
        .max();
    Ok(match active {
        Some(end) if end == NaiveDateTime::MAX => ActiveBan::Permanent,
        Some(end) => ActiveBan::Temporary(end.and_utc()),
        None => ActiveBan::NotBanned,
    })
}

pub enum ActiveBan {
    NotBanned,
    Temporary(DateTime<Utc>),
    Permanent,
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

pub trait UserSubtaskExt {
    fn is_unlocked(&self) -> bool;
    fn is_solved(&self) -> bool;
    fn is_rated(&self) -> bool;

    fn check_access(&self, user: &User, subtask: &challenges_subtasks::Model) -> bool {
        user.admin || user.id == subtask.creator || subtask.fee == 0 || self.is_unlocked()
    }

    fn can_rate(&self, user: &User, subtask: &challenges_subtasks::Model) -> bool {
        user.id != subtask.creator && self.is_solved() && !self.is_rated()
    }
}

impl UserSubtaskExt for challenges_user_subtasks::Model {
    fn is_unlocked(&self) -> bool {
        self.unlocked_timestamp.is_some()
    }

    fn is_solved(&self) -> bool {
        self.solved_timestamp.is_some()
    }

    fn is_rated(&self) -> bool {
        self.rating_timestamp.is_some()
    }
}

impl<T: UserSubtaskExt> UserSubtaskExt for &T {
    fn is_unlocked(&self) -> bool {
        T::is_unlocked(self)
    }
    fn is_solved(&self) -> bool {
        T::is_solved(self)
    }
    fn is_rated(&self) -> bool {
        T::is_rated(self)
    }
}

impl<T: UserSubtaskExt> UserSubtaskExt for Option<T> {
    fn is_unlocked(&self) -> bool {
        self.as_ref().is_some_and(|x| x.is_unlocked())
    }
    fn is_solved(&self) -> bool {
        self.as_ref().is_some_and(|x| x.is_solved())
    }
    fn is_rated(&self) -> bool {
        self.as_ref().is_some_and(|x| x.is_rated())
    }
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
