use chrono::{DateTime, TimeZone, Utc};
use lib::services;
use poem_openapi::Object;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Object, Serialize, Deserialize)]
pub struct Leaderboard {
    /// The list of users on the leaderboard in descending order of score.
    pub leaderboard: Vec<LeaderboardUser>,
    /// The total number of users on the leaderboard.
    pub total: u64,
}

#[derive(Debug, Clone, Object, Serialize, Deserialize)]
pub struct LeaderboardUser {
    pub user: Option<User>,
    #[oai(flatten)]
    pub rank: Rank,
}

#[derive(Debug, Clone, Object, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    pub avatar_url: String,
    pub registration: DateTime<Utc>,
    pub admin: bool,
}

#[derive(Debug, Clone, Object, Serialize, Deserialize)]
pub struct Rank {
    pub score: u64,
    pub rank: u64,
}

impl From<services::auth::User> for User {
    fn from(value: services::auth::User) -> Self {
        Self {
            id: value.id,
            name: value.name,
            display_name: value.display_name,
            avatar_url: value.avatar_url,
            registration: Utc.timestamp_nanos(value.registration as i64 * 1_000_000_000),
            admin: value.admin,
        }
    }
}

impl From<services::skills::Rank> for Rank {
    fn from(value: services::skills::Rank) -> Self {
        Self {
            score: value.xp,
            rank: value.rank,
        }
    }
}
