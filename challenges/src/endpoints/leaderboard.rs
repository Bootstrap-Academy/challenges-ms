use std::sync::Arc;

use chrono::{TimeZone, Utc};
use futures::future::try_join_all;
use lib::{
    auth::VerifiedUserAuth,
    services::{self, auth::User, Services},
    SharedState,
};
use poem_ext::response;
use poem_openapi::{param::Query, OpenApi};
use schemas::challenges::leaderboard::{GlobalLeaderboard, LeaderboardUser, Rank};
use uuid::Uuid;

use super::Tags;

pub struct Leaderboard {
    pub state: Arc<SharedState>,
}

#[OpenApi(tag = "Tags::Leaderboard")]
impl Leaderboard {
    #[oai(path = "/leaderboard", method = "get")]
    async fn get_leaderboard(
        &self,
        #[oai(validator(maximum(value = "100")))] limit: Query<u64>,
        offset: Query<u64>,
        _auth: VerifiedUserAuth,
    ) -> GetLeaderboard::Response<VerifiedUserAuth> {
        let leaderboard = self
            .state
            .services
            .skills
            .get_leaderboard(limit.0, offset.0)
            .await?;

        GetLeaderboard::ok(GlobalLeaderboard {
            leaderboard: try_join_all(
                leaderboard
                    .leaderboard
                    .into_iter()
                    .map(|user| resolve_user(&self.state.services, user)),
            )
            .await?,
            total: leaderboard.total,
        })
    }

    #[oai(path = "/leaderboard/:user_id", method = "get")]
    async fn get_leaderboard_user(
        &self,
        user_id: Query<Uuid>,
        _auth: VerifiedUserAuth,
    ) -> GetLeaderboardUser::Response<VerifiedUserAuth> {
        GetLeaderboardUser::ok(
            self.state
                .services
                .skills
                .get_leaderboard_user(user_id.0)
                .await?
                .into(),
        )
    }
}

response!(GetLeaderboard = {
    Ok(200) => GlobalLeaderboard,
});

response!(GetLeaderboardUser = {
    Ok(200) => Rank,
});

async fn resolve_user(
    services: &Services,
    services::skills::GlobalLeaderboardUser {
        user: user_id,
        rank,
    }: services::skills::GlobalLeaderboardUser,
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
