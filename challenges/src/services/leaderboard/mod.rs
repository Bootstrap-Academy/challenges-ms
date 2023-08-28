use chrono::{TimeZone, Utc};
use lib::services::{auth::User, Services};
use schemas::challenges::leaderboard::{LeaderboardUser, Rank};
use uuid::Uuid;

pub mod global;
pub mod language;
pub mod task;

pub async fn resolve_user(
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
