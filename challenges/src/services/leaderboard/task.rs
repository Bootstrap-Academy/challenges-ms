use entity::{challenges_subtasks, challenges_user_subtasks};
use futures::future::try_join_all;
use lib::services::Services;
use schemas::challenges::leaderboard::{Leaderboard, Rank};
use sea_orm::{
    sea_query::{Alias, BinOper, Expr},
    ColumnTrait, DatabaseTransaction, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect,
};
use uuid::Uuid;

use super::resolve_user;

pub async fn get_task_leaderboard(
    db: &DatabaseTransaction,
    services: &Services,
    task_id: Uuid,
    limit: u64,
    offset: u64,
) -> anyhow::Result<Leaderboard> {
    let base_query = challenges_user_subtasks::Entity::find()
        .select_only()
        .column(challenges_user_subtasks::Column::UserId)
        .inner_join(challenges_subtasks::Entity)
        .filter(challenges_user_subtasks::Column::SolvedTimestamp.is_not_null())
        .filter(challenges_subtasks::Column::TaskId.eq(task_id))
        .group_by(challenges_user_subtasks::Column::UserId);

    let rows = base_query
        .clone()
        .column_as(
            challenges_subtasks::Column::Xp
                .sum()
                .cast_as(Alias::new("int8")),
            "xp",
        )
        .order_by_desc(Expr::col(Alias::new("xp")))
        .order_by_asc(challenges_user_subtasks::Column::SolvedTimestamp.max())
        .limit(limit)
        .offset(offset)
        .into_tuple::<(Uuid, i64)>()
        .all(db)
        .await?;

    let total = base_query.clone().count(db).await?;

    let mut rank_xp = rows.first().map(|&(_, xp)| xp).unwrap_or(0);
    let mut rank = rank_of(db, base_query, rank_xp).await?;

    let leaderboard = rows.into_iter().enumerate().map(|(i, (id, xp))| {
        if xp < rank_xp {
            rank = offset + i as u64 + 1;
            rank_xp = xp;
        }
        (
            id,
            Rank {
                score: xp as _,
                rank,
            },
        )
    });

    Ok(Leaderboard {
        leaderboard: try_join_all(
            leaderboard.map(|(user_id, rank)| resolve_user(services, user_id, rank)),
        )
        .await?,
        total,
    })
}

pub async fn get_task_leaderboard_user(
    db: &DatabaseTransaction,
    task_id: Uuid,
    user_id: Uuid,
) -> anyhow::Result<Rank> {
    let base_query = challenges_user_subtasks::Entity::find()
        .select_only()
        .column(challenges_user_subtasks::Column::UserId)
        .inner_join(challenges_subtasks::Entity)
        .filter(challenges_user_subtasks::Column::SolvedTimestamp.is_not_null())
        .filter(challenges_subtasks::Column::TaskId.eq(task_id))
        .group_by(challenges_user_subtasks::Column::UserId);

    let xp = base_query
        .clone()
        .select_only()
        .column_as(
            challenges_subtasks::Column::Xp
                .sum()
                .cast_as(Alias::new("int8")),
            "xp",
        )
        .filter(challenges_user_subtasks::Column::UserId.eq(user_id))
        .into_tuple::<(i64,)>()
        .one(db)
        .await?
        .map(|(xp,)| xp)
        .unwrap_or(0);

    Ok(Rank {
        score: xp as _,
        rank: rank_of(db, base_query, xp).await?,
    })
}

async fn rank_of<'a, Q: QuerySelect + PaginatorTrait<'a, DatabaseTransaction> + Send>(
    db: &'a DatabaseTransaction,
    base_query: Q,
    xp: i64,
) -> anyhow::Result<u64> {
    Ok(base_query
        .having(
            challenges_subtasks::Column::Xp
                .sum()
                .binary(BinOper::GreaterThan, xp),
        )
        .count(db)
        .await?
        + 1)
}
