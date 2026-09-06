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

    let first_rank = rank_of(db, base_query, rows.first().map(|&(_, xp)| xp).unwrap_or(0)).await?;
    let ranked = assign_ranks(rows, offset, first_rank);

    Ok(Leaderboard {
        leaderboard: try_join_all(
            ranked
                .into_iter()
                .map(|(user_id, rank)| resolve_user(services, user_id, rank)),
        )
        .await?
        .into_iter()
        .flatten()
        .collect(),
        total,
    })
}

/// Assign a rank to every row of a page of a leaderboard.
///
/// `first_rank` is the rank of the first row of the page, `offset` the index of
/// that row on the whole leaderboard. Rows with an equal score share a rank and
/// the next lower score continues at its absolute position, so a rank describes
/// the position of a row on the whole leaderboard and does not depend on where
/// the page starts or on which rows are dropped from it afterwards.
fn assign_ranks(rows: Vec<(Uuid, i64)>, offset: u64, first_rank: u64) -> Vec<(Uuid, Rank)> {
    let mut rank_xp = rows.first().map(|&(_, xp)| xp).unwrap_or(0);
    let mut rank = first_rank;

    rows.into_iter()
        .enumerate()
        .map(|(i, (id, xp))| {
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
        })
        .collect()
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

/// Look up the user behind a leaderboard entry.
///
/// Returns [`None`] if the user asked not to be listed, in which case the entry
/// is dropped from the leaderboard.
async fn resolve_user(
    services: &Services,
    user_id: Uuid,
    rank: impl Into<Rank>,
) -> anyhow::Result<Option<LeaderboardUser>> {
    let user = services.auth.get_user_by_id(user_id).await?;
    Ok(leaderboard_entry(user, rank.into()))
}

/// Whether a user is listed on the leaderboards.
///
/// A user the auth microservice does not know (anymore) is listed, as before,
/// because there is no preference to honour.
fn is_listed(user: Option<&User>) -> bool {
    !user.is_some_and(|user| user.leaderboard_opt_out)
}

/// Turn a leaderboard row into an entry of the response.
///
/// Returns [`None`] if the user asked not to be listed. The entry is then
/// dropped from the page without changing the ranks of the other entries or the
/// total, so that paging by `offset` keeps working.
fn leaderboard_entry(user: Option<User>, rank: Rank) -> Option<LeaderboardUser> {
    is_listed(user.as_ref()).then(|| LeaderboardUser {
        user: user.map(Into::into),
        rank,
    })
}

/// Whether `auth` may look up the rank of `user_id` even if that user asked not
/// to be listed.
///
/// Everyone can see their own rank and administrators can see every rank.
fn may_see_hidden_rank(auth: &lib::auth::User, user_id: Uuid) -> bool {
    auth.id == user_id || auth.admin
}

/// Whether `auth` may look up the leaderboard rank of `user_id`.
///
/// Users who asked not to be listed are only visible to themselves and to
/// administrators.
pub async fn is_rank_visible(
    services: &Services,
    auth: &lib::auth::User,
    user_id: Uuid,
) -> anyhow::Result<bool> {
    if may_see_hidden_rank(auth, user_id) {
        return Ok(true);
    }
    Ok(is_listed(
        services.auth.get_user_by_id(user_id).await?.as_ref(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth_user(id: Uuid, admin: bool) -> lib::auth::User {
        lib::auth::User {
            id,
            email_verified: true,
            admin,
        }
    }

    fn user(leaderboard_opt_out: bool) -> User {
        User {
            id: Uuid::new_v4(),
            name: "user".into(),
            display_name: "User".into(),
            avatar_url: None,
            registration: 0.0,
            admin: false,
            leaderboard_opt_out,
        }
    }

    fn rank(rank: u64) -> Rank {
        Rank { score: 42, rank }
    }

    #[test]
    fn test_listed_user_is_an_entry() {
        let entry = leaderboard_entry(Some(user(false)), rank(3)).unwrap();
        assert!(entry.user.is_some());
        assert_eq!(entry.rank.rank, 3);
        assert_eq!(entry.rank.score, 42);
    }

    #[test]
    fn test_hidden_user_is_no_entry() {
        assert!(leaderboard_entry(Some(user(true)), rank(3)).is_none());
    }

    #[test]
    fn test_unknown_user_is_an_entry_without_a_user() {
        let entry = leaderboard_entry(None, rank(3)).unwrap();
        assert!(entry.user.is_none());
        assert_eq!(entry.rank.rank, 3);
    }

    #[test]
    fn test_own_rank_is_visible() {
        let id = Uuid::new_v4();
        assert!(may_see_hidden_rank(&auth_user(id, false), id));
    }

    #[test]
    fn test_admin_sees_every_rank() {
        assert!(may_see_hidden_rank(
            &auth_user(Uuid::new_v4(), true),
            Uuid::new_v4()
        ));
    }

    #[test]
    fn test_other_user_needs_the_user_to_be_listed() {
        assert!(!may_see_hidden_rank(
            &auth_user(Uuid::new_v4(), false),
            Uuid::new_v4()
        ));
        assert!(is_listed(Some(&user(false))));
        assert!(!is_listed(Some(&user(true))));
        assert!(is_listed(None));
    }

    #[test]
    fn test_assign_ranks_shares_a_rank_between_equal_scores() {
        let ids = [
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        ];
        let rows = vec![(ids[0], 30), (ids[1], 20), (ids[2], 20), (ids[3], 10)];

        let ranked = assign_ranks(rows, 0, 1);

        assert_eq!(
            ranked.iter().map(|(_, r)| r.rank).collect::<Vec<_>>(),
            [1, 2, 2, 4]
        );
        assert_eq!(
            ranked.iter().map(|(_, r)| r.score).collect::<Vec<_>>(),
            [30, 20, 20, 10]
        );
    }

    #[test]
    fn test_assign_ranks_continues_at_the_offset_of_the_page() {
        let rows = vec![(Uuid::new_v4(), 30), (Uuid::new_v4(), 20)];

        let ranked = assign_ranks(rows, 10, 7);

        assert_eq!(
            ranked.iter().map(|(_, r)| r.rank).collect::<Vec<_>>(),
            [7, 12]
        );
    }

    #[test]
    fn test_assign_ranks_of_an_empty_page() {
        assert!(assign_ranks(Vec::new(), 0, 1).is_empty());
    }

    #[test]
    fn test_hidden_user_does_not_move_the_other_entries_of_a_page() {
        let ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
        let rows = vec![(ids[0], 30), (ids[1], 20), (ids[2], 10)];
        let users = [Some(user(false)), Some(user(true)), Some(user(false))];

        let entries = assign_ranks(rows, 0, 1)
            .into_iter()
            .zip(users)
            .filter_map(|((_, rank), user)| leaderboard_entry(user, rank))
            .collect::<Vec<_>>();

        // the middle user is gone, the ranks of the others are unchanged, so
        // the next page still starts behind the third row
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.rank.rank)
                .collect::<Vec<_>>(),
            [1, 3]
        );
    }
}
