use chrono::{TimeZone, Utc};
use futures::future::try_join_all;
use lib::services::{auth::User, Services};
use schemas::challenges::leaderboard::{Leaderboard, LeaderboardUser, Rank};
use sea_orm::{
    sea_query::{Alias, BinOper, Expr, Query, SelectStatement},
    ConnectionTrait, DatabaseTransaction, Order,
};
use uuid::Uuid;

pub mod global;
pub mod language;
pub mod task;

async fn get_leaderboard(
    db: &DatabaseTransaction,
    services: &Services,
    base_query: SelectStatement,
    limit: u64,
    offset: u64,
) -> anyhow::Result<Leaderboard> {
    let rows: Vec<(Uuid, i64)> = db
        .query_all(
            db.get_database_backend().build(
                base_query
                    .clone()
                    .order_by(Alias::new("xp"), Order::Desc)
                    .order_by(Alias::new("last_update"), Order::Asc)
                    .limit(limit)
                    .offset(offset),
            ),
        )
        .await?
        .into_iter()
        .map(|row| row.try_get_many_by_index())
        .collect::<Result<_, _>>()?;

    let total = db
        .query_one(
            db.get_database_backend().build(
                Query::select()
                    .expr(Expr::col(Alias::new("user_id")).count())
                    .from_subquery(base_query.clone(), Alias::new("x")),
            ),
        )
        .await?
        .map(|row| row.try_get_many_by_index::<(i64,)>())
        .transpose()?
        .map(|(total,)| total as u64)
        .unwrap_or(0);

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

pub async fn get_leaderboard_user(
    db: &DatabaseTransaction,
    base_query: SelectStatement,
    user_id: Uuid,
) -> anyhow::Result<Rank> {
    let xp = db
        .query_one(
            db.get_database_backend().build(
                base_query
                    .clone()
                    .and_where(Expr::col(Alias::new("user_id")).eq(user_id)),
            ),
        )
        .await?
        .map(|row| row.try_get_many_by_index::<(Uuid, i64)>())
        .transpose()?
        .map(|(_, xp)| xp)
        .unwrap_or(0);

    Ok(Rank {
        score: xp as _,
        rank: rank_of(db, base_query, xp).await?,
    })
}

async fn rank_of(
    db: &DatabaseTransaction,
    mut base_query: SelectStatement,
    xp: i64,
) -> anyhow::Result<u64> {
    Ok(db
        .query_one(
            db.get_database_backend().build(
                Query::select()
                    .expr(Expr::col(Alias::new("user_id")).count())
                    .from_subquery(
                        base_query
                            .and_having(
                                Expr::col(Alias::new("xp"))
                                    .sum()
                                    .binary(BinOper::GreaterThan, xp),
                            )
                            .to_owned(),
                        Alias::new("x"),
                    ),
            ),
        )
        .await?
        .map(|row| row.try_get_many_by_index::<(i64,)>())
        .transpose()?
        .map(|(total,)| total as u64)
        .unwrap_or(0)
        + 1)
}

async fn resolve_user(
    services: &Services,
    user_id: Uuid,
    rank: impl Into<Rank>,
) -> anyhow::Result<LeaderboardUser> {
    let user = services.auth.get_user_by_id(user_id).await?;
    let (name, registration, admin) = match user {
        Some(User {
            name,
            registration,
            admin,
            ..
        }) => (
            name,
            Utc.timestamp_nanos(registration as i64 * 1_000_000_000),
            admin,
        ),
        None => ("[Deleted User]".into(), Utc.timestamp_nanos(0), false),
    };
    Ok(LeaderboardUser {
        user_id,
        name,
        registration,
        admin,
        rank: rank.into(),
    })
}
