use futures::future::try_join_all;
use lib::services::Services;
use schemas::challenges::leaderboard::{Leaderboard, Rank};
use uuid::Uuid;
use chrono::NaiveDateTime;

use super::resolve_user;

/// Get the global leaderboard with optional date range filtering
pub async fn get_global_leaderboard(
    services: &Services,
    limit: u64,
    offset: u64,
    date_range: Option<(NaiveDateTime, NaiveDateTime)>,
) -> anyhow::Result<Leaderboard> {
    let leaderboard = match date_range {
        Some((start_date, end_date)) => {
            services.skills
                .get_leaderboard_with_date_range(limit, offset, start_date, end_date)
                .await?
        }
        None => {
            services.skills
                .get_leaderboard(limit, offset)
                .await?
        }
    };

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

/// Get a specific user's global rank with optional date range filtering
pub async fn get_global_leaderboard_user(
    services: &Services,
    user_id: Uuid,
    date_range: Option<(NaiveDateTime, NaiveDateTime)>,
) -> anyhow::Result<Rank> {
    let rank = match date_range {
        Some((start_date, end_date)) => {
            services.skills
                .get_leaderboard_user_with_date_range(user_id, start_date, end_date)
                .await?
        }
        None => {
            services.skills
                .get_leaderboard_user(user_id)
                .await?
        }
    };
    
    Ok(rank.into())
}
