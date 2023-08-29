use entity::{
    challenges_coding_challenge_result, challenges_coding_challenge_submissions,
    challenges_subtasks, sea_orm_active_enums::ChallengesVerdictVariant,
};
use futures::future::try_join_all;
use lib::services::Services;
use schemas::challenges::leaderboard::{Leaderboard, Rank};
use sea_orm::{
    sea_query::{Alias, BinOper, Expr, Query, SelectStatement, SimpleExpr},
    ColumnTrait, ConnectionTrait, DatabaseTransaction, Iden, Order, Value,
};
use uuid::Uuid;

use super::resolve_user;

fn get_base_query(language: &str) -> SelectStatement {
    Query::select()
        .column(Alias::new("user_id"))
        .expr_as(
            challenges_subtasks::Column::Xp
                .sum()
                .cast_as(Alias::new("int8")),
            Alias::new("xp"),
        )
        .expr_as(
            Expr::col(Alias::new("last_update")).max(),
            Alias::new("last_update"),
        )
        .from_subquery(
            Query::select()
                .expr_as(
                    Expr::col((
                        challenges_coding_challenge_submissions::Entity,
                        challenges_coding_challenge_submissions::Column::Creator,
                    )),
                    Alias::new("user_id"),
                )
                .expr_as(
                    Expr::col(challenges_coding_challenge_submissions::Column::SubtaskId),
                    Alias::new("subtask_id"),
                )
                .expr_as(
                    Expr::col((
                        challenges_coding_challenge_submissions::Entity,
                        challenges_coding_challenge_submissions::Column::CreationTimestamp,
                    ))
                    .max(),
                    Alias::new("last_update"),
                )
                .from(challenges_coding_challenge_result::Entity)
                .inner_join(
                    challenges_coding_challenge_submissions::Entity,
                    Expr::col((
                        challenges_coding_challenge_result::Entity,
                        challenges_coding_challenge_result::Column::SubmissionId,
                    ))
                    .equals((
                        challenges_coding_challenge_submissions::Entity,
                        challenges_coding_challenge_submissions::Column::Id,
                    )),
                )
                .inner_join(
                    challenges_subtasks::Entity,
                    Expr::col((challenges_subtasks::Entity, challenges_subtasks::Column::Id))
                        .equals((
                            challenges_coding_challenge_submissions::Entity,
                            challenges_coding_challenge_submissions::Column::SubtaskId,
                        )),
                )
                .and_where(
                    Expr::col(challenges_coding_challenge_submissions::Column::Environment)
                        .eq(language),
                )
                .and_where(
                    Expr::col(challenges_coding_challenge_result::Column::Verdict).eq(
                        SimpleExpr::Constant(Value::String(Some(
                            ChallengesVerdictVariant::Ok.to_string().into(),
                        ))),
                    ),
                )
                .group_by_columns([
                    (
                        challenges_coding_challenge_submissions::Entity,
                        challenges_coding_challenge_submissions::Column::Creator,
                    ),
                    (
                        challenges_coding_challenge_submissions::Entity,
                        challenges_coding_challenge_submissions::Column::SubtaskId,
                    ),
                ])
                .to_owned(),
            Alias::new("x"),
        )
        .inner_join(
            challenges_subtasks::Entity,
            Expr::col(Alias::new("subtask_id")).equals(challenges_subtasks::Column::Id),
        )
        .group_by_col(Alias::new("user_id"))
        .to_owned()
}

pub async fn get_language_leaderboard(
    db: &DatabaseTransaction,
    services: &Services,
    language: &str,
    limit: u64,
    offset: u64,
) -> anyhow::Result<Leaderboard> {
    let base_query = get_base_query(language);

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

pub async fn get_language_leaderboard_user(
    db: &DatabaseTransaction,
    language: &str,
    user_id: Uuid,
) -> anyhow::Result<Rank> {
    let base_query = get_base_query(language);

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
