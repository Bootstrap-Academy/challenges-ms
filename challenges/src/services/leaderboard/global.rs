use futures::future::try_join_all;
use lib::services::Services;
use schemas::challenges::leaderboard::{Leaderboard, Rank};
use uuid::Uuid;

use super::resolve_user;

pub async fn get_global_leaderboard(
    services: &Services,
    limit: u64,
    offset: u64,
) -> anyhow::Result<Leaderboard> {
    let leaderboard = services.skills.get_leaderboard(limit, offset).await?;

    Ok(Leaderboard {
        leaderboard: try_join_all(
            leaderboard
                .leaderboard
                .into_iter()
                .map(|user| resolve_user(services, user.user, user.rank)),
        )
        .await?,
        total: leaderboard.total,
    })
}

pub async fn get_global_leaderboard_user(
    services: &Services,
    user_id: Uuid,
) -> anyhow::Result<Rank> {
    Ok(services.skills.get_leaderboard_user(user_id).await?.into())
}
