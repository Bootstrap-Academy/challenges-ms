use std::collections::HashMap;

use chrono::{DateTime, NaiveDateTime, Utc};
use entity::{
    challenges_ban, challenges_subtasks, challenges_tasks, challenges_user_subtasks,
    sea_orm_active_enums::{ChallengesBanAction, ChallengesSubtaskType},
};
use lib::{
    auth::User,
    config::Config,
    services::{
        shop::AddCoinsError, skills::AddSkillProgressError, ServiceError, ServiceResult, Services,
    },
};
use poem_ext::responses::ErrorResponse;
use schemas::challenges::subtasks::{
    CreateSubtaskRequest, Subtask, SubtaskStats, UpdateSubtaskRequest,
};
use sea_orm::{
    sea_query::Expr, ActiveModelTrait, ColumnTrait, DatabaseTransaction, DbErr, EntityTrait,
    ModelTrait, QueryFilter, QueryOrder, Related, Set, Unchanged,
};
use thiserror::Error;
use uuid::Uuid;

use super::{
    course_tasks::get_skills_of_course,
    tasks::{get_specific_task, get_task, get_task_with_specific, Task},
};

pub async fn check_hearts(
    services: &Services,
    _config: &Config,
    user: &User,
    subtask: &challenges_subtasks::Model,
) -> anyhow::Result<bool> {
    Ok(super::hearts::admit(services, user, subtask)
        .await?
        .is_some())
}

pub async fn send_task_rewards(
    services: &Services,
    db: &DatabaseTransaction,
    user_id: Uuid,
    subtask: &challenges_subtasks::Model,
) -> Result<(), SendTaskRewardsError> {
    if subtask.retired {
        return Ok(());
    }

    let skills = if subtask.xp != 0 {
        get_skills(
            services,
            get_parent_task(db, subtask)
                .await?
                .ok_or(SendTaskRewardsError::NoParentTask)?
                .1,
        )
        .await?
    } else {
        Vec::new()
    };
    // No remote side effect until the first completion and this exact outbox
    // commit together. Empty skill resolution preserves the former zero awards.
    super::benefits::record(db, user_id, subtask.id, subtask.xp, subtask.coins, skills).await?;

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
        .filter(challenges_ban::Column::Rescinded.eq(false))
        .filter(Expr::cust("NOT EXISTS (SELECT 1 FROM moderation_holds h WHERE h.case_id=challenges_ban.id AND NOT h.active)"))
        .filter(challenges_ban::Column::Start.lte(Utc::now().naive_utc()))
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
    fn is_solved(&self) -> bool;
    fn is_rated(&self) -> bool;
    fn last_attempt(&self) -> Option<DateTime<Utc>>;
    fn attempts(&self) -> usize;

    fn can_rate(&self, user: &User, subtask: &challenges_subtasks::Model) -> bool {
        user.id != subtask.creator && self.is_solved() && !self.is_rated()
    }

    fn can_report(&self, user: &User, subtask: &challenges_subtasks::Model) -> bool {
        user.id != subtask.creator && !self.is_rated()
    }

    fn attempted(&self) -> bool {
        self.last_attempt().is_some()
    }
}

impl UserSubtaskExt for challenges_user_subtasks::Model {
    fn is_solved(&self) -> bool {
        self.solved_timestamp.is_some()
    }

    fn is_rated(&self) -> bool {
        self.rating_timestamp.is_some()
    }

    fn last_attempt(&self) -> Option<DateTime<Utc>> {
        self.last_attempt_timestamp.map(|x| x.and_utc())
    }

    fn attempts(&self) -> usize {
        self.attempts as _
    }
}

impl<T: UserSubtaskExt> UserSubtaskExt for &T {
    fn is_solved(&self) -> bool {
        T::is_solved(self)
    }
    fn is_rated(&self) -> bool {
        T::is_rated(self)
    }
    fn last_attempt(&self) -> Option<DateTime<Utc>> {
        T::last_attempt(self)
    }
    fn attempts(&self) -> usize {
        T::attempts(self)
    }
}

impl<T: UserSubtaskExt> UserSubtaskExt for Option<T> {
    fn is_solved(&self) -> bool {
        self.as_ref().is_some_and(|x| x.is_solved())
    }
    fn is_rated(&self) -> bool {
        self.as_ref().is_some_and(|x| x.is_rated())
    }
    fn last_attempt(&self) -> Option<DateTime<Utc>> {
        self.as_ref().and_then(|x| x.last_attempt())
    }
    fn attempts(&self) -> usize {
        self.as_ref().map(|x| x.attempts()).unwrap_or(0)
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

#[derive(Default)]
pub struct QuerySubtasksFilter {
    pub attempted: Option<bool>,
    pub solved: Option<bool>,
    pub rated: Option<bool>,
    pub enabled: Option<bool>,
    pub retired: Option<bool>,
    pub creator: Option<Uuid>,
    pub ty: Option<ChallengesSubtaskType>,
}

pub async fn query_subtasks_only(
    db: &DatabaseTransaction,
    user: &User,
    task_id: Option<Uuid>,
    filter: QuerySubtasksFilter,
) -> Result<Vec<Subtask>, DbErr> {
    let user_subtasks = get_user_subtasks(db, user.id).await?;
    let mut query = challenges_subtasks::Entity::find();
    if let Some(task_id) = task_id {
        query = query.filter(challenges_subtasks::Column::TaskId.eq(task_id));
    }
    Ok(super::moderation::effective_subtasks(
        db,
        prepare_query(query, &filter, user).all(db).await?,
    )
    .await?
    .into_iter()
    .filter(|subtask| effective_filter(subtask, &filter, user))
    .filter_map(|subtask| subtasks_filter_map(subtask, &filter, &user_subtasks))
    .collect())
}

pub async fn stat_subtasks_prepare(
    db: &DatabaseTransaction,
    user: &User,
    task_ids: Option<Vec<Uuid>>,
    filter: &QuerySubtasksFilter,
) -> Result<Vec<challenges_subtasks::Model>, DbErr> {
    let mut query = challenges_subtasks::Entity::find();
    if let Some(task_ids) = task_ids {
        query = query.filter(challenges_subtasks::Column::TaskId.is_in(task_ids));
    }
    Ok(
        super::moderation::effective_subtasks(
            db,
            prepare_query(query, filter, user).all(db).await?,
        )
        .await?
        .into_iter()
        .filter(|s| effective_filter(s, filter, user))
        .collect(),
    )
}

pub fn stat_subtasks(
    subtasks: &[challenges_subtasks::Model],
    user_subtasks: &HashMap<Uuid, challenges_user_subtasks::Model>,
    filter: QuerySubtasksFilter,
) -> SubtaskStats {
    let mut total = 0;
    let mut solved = 0;
    let mut attempted = 0;

    for subtask in subtasks {
        let user_subtask = user_subtasks.get(&subtask.id);
        if !subtasks_filter(subtask, &filter, user_subtask) {
            continue;
        }

        total += 1;
        solved += user_subtask.is_solved() as u64;
        attempted += (!user_subtask.is_solved() && user_subtask.attempted()) as u64;
    }

    let unattempted = total - solved - attempted;

    SubtaskStats {
        total,
        solved,
        attempted,
        unattempted,
    }
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
    let user_subtasks = get_user_subtasks(db, user.id).await?;
    let pairs = prepare_query(
        E::find()
            .find_also_related(challenges_subtasks::Entity)
            .filter(challenges_subtasks::Column::TaskId.eq(task_id)),
        &filter,
        user,
    )
    .all(db)
    .await?;
    let mut effective: HashMap<_, _> = super::moderation::effective_subtasks(
        db,
        pairs.iter().filter_map(|(_, s)| s.clone()).collect(),
    )
    .await?
    .into_iter()
    .map(|s| (s.id, s))
    .collect();
    Ok(pairs
        .into_iter()
        .filter_map(|(specific, subtask)| {
            let subtask = effective.remove(&subtask?.id)?;
            if !effective_filter(&subtask, &filter, user) {
                return None;
            }
            Some(map(
                specific,
                subtasks_filter_map(subtask, &filter, &user_subtasks)?,
            ))
        })
        .collect())
}

fn prepare_query<Q>(mut query: Q, filter: &QuerySubtasksFilter, _user: &User) -> Q
where
    Q: QueryFilter + QueryOrder,
{
    // Candidate selection is deliberately independent of mutable moderation.
    // These existing lists are unpaginated. Project all candidates once, then
    // apply both membership and flags at that same final read boundary.
    if let Some(creator) = filter.creator {
        query = query.filter(challenges_subtasks::Column::Creator.eq(creator));
    }
    if let Some(ty) = filter.ty {
        query = query.filter(challenges_subtasks::Column::Ty.eq(ty));
    }
    query.order_by_asc(challenges_subtasks::Column::CreationTimestamp)
}

fn effective_filter(
    s: &challenges_subtasks::Model,
    filter: &QuerySubtasksFilter,
    user: &User,
) -> bool {
    (user.admin || (!s.moderation_removed && (user.id == s.creator || s.enabled)))
        && filter.enabled.map_or(true, |value| value == s.enabled)
        && filter.retired.map_or(true, |value| value == s.retired)
}

fn subtasks_filter(
    subtask: &challenges_subtasks::Model,
    filter: &QuerySubtasksFilter,
    user_subtask: Option<&challenges_user_subtasks::Model>,
) -> bool {
    let attempted = user_subtask.attempted();
    let solved = user_subtask.is_solved();
    let rated = user_subtask.is_rated();
    !filter.ty.is_some_and(|ty| ty != subtask.ty)
        && filter.attempted.unwrap_or(attempted) == attempted
        && filter.solved.unwrap_or(solved) == solved
        && filter.rated.unwrap_or(rated) == rated
}

fn subtasks_filter_map(
    subtask: challenges_subtasks::Model,
    filter: &QuerySubtasksFilter,
    user_subtasks: &HashMap<Uuid, challenges_user_subtasks::Model>,
) -> Option<Subtask> {
    let user_subtask = user_subtasks.get(&subtask.id);
    let attempted = user_subtask.attempted();
    let solved = user_subtask.is_solved();
    let rated = user_subtask.is_rated();
    (filter.attempted.unwrap_or(attempted) == attempted
        && filter.solved.unwrap_or(solved) == solved
        && filter.rated.unwrap_or(rated) == rated)
        .then_some(Subtask::from(subtask, solved, rated))
}

pub async fn query_subtask<E, T>(
    db: &DatabaseTransaction,
    user: &User,
    task_id: Uuid,
    subtask_id: Uuid,
    map: impl Fn(E::Model, Subtask) -> T,
) -> Result<Option<T>, DbErr>
where
    E: EntityTrait + Related<challenges_subtasks::Entity>,
    E::PrimaryKey: sea_orm::PrimaryKeyTrait<ValueType = Uuid>,
{
    let Some((specific, subtask)) = get_subtask::<E>(db, task_id, subtask_id).await? else {
        return Ok(None);
    };
    if !user.admin
        && (subtask.moderation_removed || (user.id != subtask.creator && !subtask.enabled))
    {
        return Ok(None);
    }

    let user_subtask = get_user_subtask(db, user.id, subtask.id).await?;

    Ok(Some(map(
        specific,
        Subtask::from(subtask, user_subtask.is_solved(), user_subtask.is_rated()),
    )))
}

pub async fn query_subtask_admin<E, T>(
    db: &DatabaseTransaction,
    user: &User,
    task_id: Uuid,
    subtask_id: Uuid,
    map: impl Fn(E::Model, Subtask) -> T,
) -> Result<Result<T, QuerySubtaskAdminError>, DbErr>
where
    E: EntityTrait + Related<challenges_subtasks::Entity>,
    E::PrimaryKey: sea_orm::PrimaryKeyTrait<ValueType = Uuid>,
{
    let Some((specific, subtask)) = get_subtask::<E>(db, task_id, subtask_id).await? else {
        return Ok(Err(QuerySubtaskAdminError::NotFound));
    };

    if !(user.admin || user.id == subtask.creator) {
        return Ok(Err(QuerySubtaskAdminError::NoAccess));
    }

    let user_subtask = get_user_subtask(db, user.id, subtask.id).await?;
    Ok(Ok(map(
        specific,
        Subtask::from(subtask, user_subtask.is_solved(), user_subtask.is_rated()),
    )))
}

pub enum QuerySubtaskAdminError {
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
            Some((specific, Some(subtask))) => Some((
                specific,
                super::moderation::effective_subtask(db, subtask).await?,
            )),
            _ => None,
        },
    )
}

pub async fn create_subtask(
    db: &DatabaseTransaction,
    config: &Config,
    user: &User,
    task_id: Uuid,
    data: CreateSubtaskRequest,
    ty: ChallengesSubtaskType,
) -> Result<Result<Subtask, CreateSubtaskError>, ErrorResponse> {
    if !user.admin {
        return Ok(Err(CreateSubtaskError::Forbidden));
    }
    let (task, _specific) = match get_task_with_specific(db, task_id).await? {
        Some(task) => task,
        None => return Ok(Err(CreateSubtaskError::TaskNotFound)),
    };

    let xp = data.xp.unwrap_or(config.challenges.quizzes.max_xp);
    // Rewards are XP only, regardless of an older configured coin default.
    let coins = data.coins.unwrap_or(0);
    if coins != 0 {
        return Ok(Err(CreateSubtaskError::CoinLimitExceeded(0)));
    }
    match get_active_ban(db, user, ChallengesBanAction::Create).await? {
        ActiveBan::NotBanned => {}
        ActiveBan::Temporary(end) => return Ok(Err(CreateSubtaskError::Banned(Some(end)))),
        ActiveBan::Permanent => return Ok(Err(CreateSubtaskError::Banned(None))),
    }

    let subtask = challenges_subtasks::ActiveModel {
        id: Set(Uuid::new_v4()),
        task_id: Set(task.id),
        ty: Set(ty),
        creator: Set(user.id),
        creation_timestamp: Set(Utc::now().naive_utc()),
        xp: Set(xp as _),
        coins: Set(coins as _),
        enabled: Set(true),
        retired: Set(false),
        moderation_removed: Set(false),
    }
    .insert(db)
    .await?;

    Ok(Ok(Subtask::from(subtask, false, false)))
}

pub enum CreateSubtaskError {
    TaskNotFound,
    Forbidden,
    Banned(Option<DateTime<Utc>>),
    CoinLimitExceeded(u64),
}

pub async fn update_subtask<E>(
    db: &DatabaseTransaction,
    user: &User,
    task_id: Uuid,
    subtask_id: Uuid,
    data: UpdateSubtaskRequest,
) -> Result<Result<(E::Model, Subtask), UpdateSubtaskError>, DbErr>
where
    E: EntityTrait + Related<challenges_subtasks::Entity>,
    E::PrimaryKey: sea_orm::PrimaryKeyTrait<ValueType = Uuid>,
{
    if !user.admin {
        return Ok(Err(UpdateSubtaskError::Forbidden));
    }
    super::moderation::lock_subtask(db, subtask_id).await?;
    let Some((specific, subtask)) = get_subtask::<E>(db, task_id, subtask_id).await? else {
        return Ok(Err(UpdateSubtaskError::SubtaskNotFound));
    };

    if get_task(db, *data.task_id.get_new(&subtask.task_id))
        .await?
        .is_none()
    {
        return Ok(Err(UpdateSubtaskError::TaskNotFound));
    };

    let subtask = challenges_subtasks::ActiveModel {
        id: Unchanged(subtask.id),
        task_id: data.task_id.update(subtask.task_id),
        ty: Unchanged(subtask.ty),
        creator: Unchanged(subtask.creator),
        creation_timestamp: Unchanged(subtask.creation_timestamp),
        xp: data.xp.map(|x| x as _).update(subtask.xp),
        coins: data.coins.map(|x| x as _).update(subtask.coins),
        enabled: data.enabled.update(subtask.enabled),
        retired: data.retired.update(subtask.retired),
        moderation_removed: Unchanged(subtask.moderation_removed),
    }
    .update(db)
    .await?;

    let user_subtask = get_user_subtask(db, user.id, subtask.id).await?;
    Ok(Ok((
        specific,
        Subtask::from(subtask, user_subtask.is_solved(), user_subtask.is_rated()),
    )))
}

pub enum UpdateSubtaskError {
    Forbidden,
    SubtaskNotFound,
    TaskNotFound,
}
