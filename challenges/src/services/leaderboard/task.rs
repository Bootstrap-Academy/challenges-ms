use entity::{challenges_subtasks, challenges_user_subtasks};
use lib::services::Services;
use schemas::challenges::leaderboard::{Leaderboard, Rank};
use sea_orm::{
    sea_query::{Alias, Expr, Query, SelectStatement},
    ColumnTrait, DatabaseTransaction,
};
use uuid::Uuid;
use chrono::NaiveDateTime;

use super::{get_leaderboard, get_leaderboard_user};

fn get_base_query(task_id: Uuid, date_range: Option<(NaiveDateTime, NaiveDateTime)>) -> SelectStatement {
    let mut query = Query::select()
        .column(Alias::new("user_id"))
        .expr_as(
            challenges_subtasks::Column::Xp
                .sum()
                .cast_as(Alias::new("int8")),
            Alias::new("xp"),
        )
        .expr_as(
            challenges_user_subtasks::Column::SolvedTimestamp.max(),
            Alias::new("last_update"),
        )
        .from(challenges_user_subtasks::Entity)
        .inner_join(
            challenges_subtasks::Entity,
            Expr::col((challenges_subtasks::Entity, challenges_subtasks::Column::Id)).equals((
                challenges_user_subtasks::Entity,
                challenges_user_subtasks::Column::SubtaskId,
            )),
        )
        .and_where(challenges_user_subtasks::Column::SolvedTimestamp.is_not_null())
        .and_where(challenges_subtasks::Column::TaskId.eq(task_id));

    // Add date range filter if provided
    if let Some((start_date, end_date)) = date_range {
        query = query.and_where(
            challenges_user_subtasks::Column::SolvedTimestamp.between(start_date, end_date)
        );
    }

    query.group_by_col(challenges_user_subtasks::Column::UserId)
        .to_owned()
}

pub async fn get_task_leaderboard(
    db: &DatabaseTransaction,
    services: &Services,
    task_id: Uuid,
    limit: u64,
    offset: u64,
    date_range: Option<(NaiveDateTime, NaiveDateTime)>,
) -> anyhow::Result<Leaderboard> {
    let base_query = get_base_query(task_id, date_range);
    get_leaderboard(db, services, base_query, limit, offset).await
}

pub async fn get_task_leaderboard_user(
    db: &DatabaseTransaction,
    task_id: Uuid,
    user_id: Uuid,
    date_range: Option<(NaiveDateTime, NaiveDateTime)>,
) -> anyhow::Result<Rank> {
    let base_query = get_base_query(task_id, date_range);
    get_leaderboard_user(db, base_query, user_id).await
}