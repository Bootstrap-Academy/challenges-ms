use std::sync::Arc;

use lib::{auth::VerifiedUserAuth, config::Config};
use poem_ext::response;
use poem_openapi::OpenApi;
use schemas::challenges::subtasks::SubtasksUserConfig;

use crate::endpoints::Tags;

pub struct Api {
    pub config: Arc<Config>,
}

#[OpenApi(tag = "Tags::Subtasks")]
impl Api {
    /// Return the configuration values that are relevant for normal users
    /// creating subtasks.
    #[oai(path = "/subtasks/user_config", method = "get")]
    pub async fn get_user_config(
        &self,
        _auth: VerifiedUserAuth,
    ) -> GetUserConfig::Response<VerifiedUserAuth> {
        GetUserConfig::ok(SubtasksUserConfig {
            min_level: self.config.challenges.quizzes.min_level,
            max_xp: self.config.challenges.quizzes.max_xp,
            max_coins: self.config.challenges.quizzes.max_coins,
            max_fee: self.config.challenges.quizzes.max_fee,
        })
    }
}

response!(GetUserConfig = {
    Ok(200) => SubtasksUserConfig,
});
