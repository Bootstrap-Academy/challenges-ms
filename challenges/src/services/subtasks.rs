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
use poem_ext::responses::ErrorResponse;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, DbErr, EntityTrait, ModelTrait,
    QueryFilter, QueryOrder, Related, Set, Unchanged,
};
use thiserror::Error;
use uuid::Uuid;

use super::{
    course_tasks::get_skills_of_course,
    tasks::{get_specific_task, get_task_with_specific, Task},
};
use crate::schemas::subtasks::{CreateSubtaskRequest, Subtask};

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

pub struct QuerySubtasksFilter {
    pub free: Option<bool>,
    pub unlocked: Option<bool>,
    pub solved: Option<bool>,
    pub rated: Option<bool>,
    pub enabled: Option<bool>,
}

pub async fn query_subtasks<E, T>(
    db: &DatabaseTransaction,
    user: &User,
    task_id: Uuid,
    filter: QuerySubtasksFilter,
    map: impl Fn(E::Model, Subtask) -> T,
) -> Result<Vec<T>, DbErr>
where
    E: EntityTrait + Related<challenges_subtasks::Entity>,
{
    let subtasks = get_user_subtasks(db, user.id).await?;
    Ok(E::find()
        .find_also_related(challenges_subtasks::Entity)
        .filter(challenges_subtasks::Column::TaskId.eq(task_id))
        .order_by_asc(challenges_subtasks::Column::CreationTimestamp)
        .all(db)
        .await?
        .into_iter()
        .filter_map(|(specific, subtask)| {
            let subtask = subtask?;
            let id = subtask.id;
            let free = subtask.fee <= 0;
            let unlocked = subtasks.get(&id).check_access(user, &subtask);
            let solved = subtasks.get(&id).is_solved();
            let rated = subtasks.get(&id).is_rated();
            let enabled = subtask.enabled;
            ((user.admin || user.id == subtask.creator || subtask.enabled)
                && filter.free.unwrap_or(free) == free
                && filter.unlocked.unwrap_or(unlocked) == unlocked
                && filter.solved.unwrap_or(solved) == solved
                && filter.rated.unwrap_or(rated) == rated
                && filter.enabled.unwrap_or(enabled) == enabled)
                .then_some(map(
                    specific,
                    Subtask::from(subtask, unlocked, solved, rated),
                ))
        })
        .collect())
}

pub async fn query_subtask<E, T>(
    db: &DatabaseTransaction,
    user: &User,
    task_id: Uuid,
    subtask_id: Uuid,
    map: impl Fn(E::Model, Subtask) -> T,
) -> Result<Result<T, QuerySubtaskError>, DbErr>
where
    E: EntityTrait + Related<challenges_subtasks::Entity>,
    E::PrimaryKey: sea_orm::PrimaryKeyTrait<ValueType = Uuid>,
{
    let Some((mcq, subtask)) = get_subtask::<E>(db, task_id, subtask_id).await? else {
        return Ok(Err(QuerySubtaskError::NotFound));
    };
    if !user.admin && user.id != subtask.creator && !subtask.enabled {
        return Ok(Err(QuerySubtaskError::NotFound));
    }

    let user_subtask = get_user_subtask(db, user.id, subtask.id).await?;
    if !user_subtask.check_access(user, &subtask) {
        return Ok(Err(QuerySubtaskError::NoAccess));
    }

    Ok(Ok(map(
        mcq,
        Subtask::from(
            subtask,
            true,
            user_subtask.is_solved(),
            user_subtask.is_rated(),
        ),
    )))
}

pub async fn query_subtask_admin<E, T>(
    db: &DatabaseTransaction,
    user: &User,
    task_id: Uuid,
    subtask_id: Uuid,
    map: impl Fn(E::Model, Subtask) -> T,
) -> Result<Result<T, QuerySubtaskError>, DbErr>
where
    E: EntityTrait + Related<challenges_subtasks::Entity>,
    E::PrimaryKey: sea_orm::PrimaryKeyTrait<ValueType = Uuid>,
{
    let Some((mcq, subtask)) = get_subtask::<E>(db, task_id, subtask_id).await? else {
        return Ok(Err(QuerySubtaskError::NotFound));
    };

    if !(user.admin || user.id == subtask.creator) {
        return Ok(Err(QuerySubtaskError::NoAccess));
    }

    let user_subtask = get_user_subtask(db, user.id, subtask.id).await?;
    Ok(Ok(map(
        mcq,
        Subtask::from(
            subtask,
            true,
            user_subtask.is_solved(),
            user_subtask.is_rated(),
        ),
    )))
}

pub enum QuerySubtaskError {
    NotFound,
    NoAccess,
}

pub async fn get_subtask<E>(
    db: &DatabaseTransaction,
    task_id: Uuid,
    subtask_id: Uuid,
) -> Result<Option<(E::Model, challenges_subtasks::Model)>, DbErr>
where
    E: EntityTrait + Related<challenges_subtasks::Entity>,
    E::PrimaryKey: sea_orm::PrimaryKeyTrait<ValueType = Uuid>,
{
    Ok(
        match E::find_by_id(subtask_id)
            .find_also_related(challenges_subtasks::Entity)
            .filter(challenges_subtasks::Column::TaskId.eq(task_id))
            .one(db)
            .await?
        {
            Some((specific, Some(subtask))) => Some((specific, subtask)),
            _ => None,
        },
    )
}

pub async fn create_subtask(
    db: &DatabaseTransaction,
    services: &Services,
    config: &Config,
    user: &User,
    task_id: Uuid,
    data: CreateSubtaskRequest,
) -> Result<Result<Subtask, CreateSubtaskError>, ErrorResponse> {
    let (task, specific) = match get_task_with_specific(db, task_id).await? {
        Some(task) => task,
        None => return Ok(Err(CreateSubtaskError::TaskNotFound)),
    };
    if !can_create(services, config, &specific, user).await? {
        return Ok(Err(CreateSubtaskError::Forbidden));
    }

    let xp = data.xp.unwrap_or(config.challenges.quizzes.max_xp);
    let coins = data.coins.unwrap_or(config.challenges.quizzes.max_coins);
    if matches!(specific, Task::CourseTask(_)) && !user.admin {
        if xp > config.challenges.quizzes.max_xp {
            return Ok(Err(CreateSubtaskError::XpLimitExceeded(
                config.challenges.quizzes.max_xp,
            )));
        }
        if coins > config.challenges.quizzes.max_coins {
            return Ok(Err(CreateSubtaskError::CoinLimitExceeded(
                config.challenges.quizzes.max_coins,
            )));
        }
        if data.fee > config.challenges.quizzes.max_fee {
            return Ok(Err(CreateSubtaskError::FeeLimitExceeded(
                config.challenges.quizzes.max_fee,
            )));
        }
    }

    match get_active_ban(db, user, ChallengesBanAction::Create).await? {
        ActiveBan::NotBanned => {}
        ActiveBan::Temporary(end) => return Ok(Err(CreateSubtaskError::Banned(Some(end)))),
        ActiveBan::Permanent => return Ok(Err(CreateSubtaskError::Banned(None))),
    }

    let subtask = challenges_subtasks::ActiveModel {
        id: Set(Uuid::new_v4()),
        task_id: Set(task.id),
        creator: Set(user.id),
        creation_timestamp: Set(Utc::now().naive_utc()),
        xp: Set(xp as _),
        coins: Set(coins as _),
        fee: Set(data.fee as _),
        enabled: Set(true),
    }
    .insert(db)
    .await?;

    Ok(Ok(Subtask::from(subtask, true, false, false)))
}

pub enum CreateSubtaskError {
    TaskNotFound,
    Forbidden,
    Banned(Option<DateTime<Utc>>),
    XpLimitExceeded(u64),
    CoinLimitExceeded(u64),
    FeeLimitExceeded(u64),
}
