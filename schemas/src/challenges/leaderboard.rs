use chrono::{DateTime, Utc};
use lib::services;
use poem_openapi::Object;
use uuid::Uuid;

#[derive(Debug, Clone, Object)]
pub struct Leaderboard {
    /// The list of users on the leaderboard in descending order of score.
    pub leaderboard: Vec<LeaderboardUser>,
    /// The total number of users on the leaderboard.
    pub total: u64,
}

#[derive(Debug, Clone, Object)]
pub struct LeaderboardUser {
    pub user_id: Uuid,
    pub name: String,
    pub registration: DateTime<Utc>,
    pub admin: bool,
    #[oai(flatten)]
    pub rank: Rank,
}

#[derive(Debug, Clone, Object)]
pub struct Rank {
    pub score: u64,
    pub rank: u64,
}

impl From<services::skills::Rank> for Rank {
    fn from(value: services::skills::Rank) -> Self {
        Self {
            score: value.xp,
            rank: value.rank,
        }
    }
}
