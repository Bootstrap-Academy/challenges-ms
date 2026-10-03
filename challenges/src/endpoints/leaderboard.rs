use std::{sync::Arc, time::Duration};

use fnct::{format::JsonFormatter, key};
use lib::{auth::VerifiedUserAuth, config::Config, Cache, SharedState};
use poem::web::Data;
use poem_ext::{db::DbTxn, response};
use poem_openapi::{
    param::{Path, Query},
    OpenApi,
};
use schemas::challenges::leaderboard::{LeaderboardResponse, RankResponse};
use uuid::Uuid;

use super::Tags;
use crate::services::leaderboard::{
    global::{get_global_leaderboard, get_global_leaderboard_user},
    is_rank_visible,
    language::{get_language_leaderboard, get_language_leaderboard_user},
    published::{self, Ranking},
    task::{get_task_leaderboard, get_task_leaderboard_user},
};

pub struct LeaderboardEndpoints {
    pub state: Arc<SharedState>,
    pub cache: Cache<JsonFormatter>,
    pub config: Arc<Config>,
}

#[OpenApi(tag = "Tags::Leaderboard")]
impl LeaderboardEndpoints {
    /// Return the global leaderboard.
    ///
    /// With profile publications enabled, only current shared participants
    /// contribute to ranks and total, before pagination. Responses carry
    /// scope and epoch metadata; a stale publication_epoch returns 409.
    ///
    /// In legacy mode, users who asked not to be listed are omitted, so a page may contain
    /// fewer entries than requested and the ranks of the remaining users may
    /// have gaps. `total` counts all positions on the leaderboard, including
    /// the omitted ones, so that pagination by `offset` stays exact.
    #[oai(path = "/leaderboard", method = "get")]
    async fn get_leaderboard(
        &self,
        #[oai(validator(maximum(value = "100")))] limit: Query<u64>,
        offset: Query<u64>,
        publication_epoch: Query<Option<Uuid>>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetLeaderboard::Response<VerifiedUserAuth> {
        match published::use_shared(
            &self.state.services,
            self.config.challenges.profile_publications_enabled,
        )
        .await
        {
            Ok(true) => {
                if !auth.0.email_verified {
                    return published::Error::Unverified.response();
                }
                return match published::leaderboard(
                    &db,
                    &self.state.services,
                    &self.cache,
                    Ranking::Global,
                    limit.0,
                    offset.0,
                    publication_epoch.0,
                )
                .await
                {
                    Ok(value) => published::no_store(GetLeaderboard::ok(
                        LeaderboardResponse::Published(value),
                    )),
                    Err(error) => error.response(),
                };
            }
            Err(error) => return error.response(),
            Ok(false) => {}
        }
        GetLeaderboard::ok(
            get_global_leaderboard(&self.state.services, limit.0, offset.0)
                .await?
                .into(),
        )
    }

    /// Return the rank of a user on the global leaderboard.
    #[oai(path = "/leaderboard/:user_id", method = "get")]
    async fn get_leaderboard_user(
        &self,
        user_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetLeaderboardUser::Response<VerifiedUserAuth> {
        match published::use_shared(
            &self.state.services,
            self.config.challenges.profile_publications_enabled,
        )
        .await
        {
            Ok(true) => {
                if !auth.0.email_verified {
                    return published::Error::Unverified.response();
                }
                return match published::rank(
                    &db,
                    &self.state.services,
                    Ranking::Global,
                    user_id.0,
                    &auth.0,
                )
                .await
                {
                    Ok(value) => {
                        published::no_store(GetLeaderboardUser::ok(RankResponse::Published(value)))
                    }
                    Err(error) => error.response(),
                };
            }
            Err(error) => return error.response(),
            Ok(false) => {}
        }
        if !is_rank_visible(&self.state.services, &auth.0, user_id.0).await? {
            return GetLeaderboardUser::forbidden();
        }
        GetLeaderboardUser::ok(
            get_global_leaderboard_user(&self.state.services, user_id.0)
                .await?
                .into(),
        )
    }

    /// Return the leaderboard of a task.
    ///
    /// See the global leaderboard for publication scope, epoch pagination
    /// and the unchanged legacy treatment of omitted users.
    #[oai(path = "/leaderboard/by-task/:task_id", method = "get")]
    async fn get_task_leaderboard(
        &self,
        task_id: Path<Uuid>,
        #[oai(validator(maximum(value = "100")))] limit: Query<u64>,
        offset: Query<u64>,
        publication_epoch: Query<Option<Uuid>>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetTaskLeaderboard::Response<VerifiedUserAuth> {
        match published::use_shared(
            &self.state.services,
            self.config.challenges.profile_publications_enabled,
        )
        .await
        {
            Ok(true) => {
                if !auth.0.email_verified {
                    return published::Error::Unverified.response();
                }
                return match published::leaderboard(
                    &db,
                    &self.state.services,
                    &self.cache,
                    Ranking::Task(task_id.0),
                    limit.0,
                    offset.0,
                    publication_epoch.0,
                )
                .await
                {
                    Ok(value) => published::no_store(GetTaskLeaderboard::ok(
                        LeaderboardResponse::Published(value),
                    )),
                    Err(error) => error.response(),
                };
            }
            Err(error) => return error.response(),
            Ok(false) => {}
        }
        let leaderboard = self
            .cache
            .cached_result(
                key!(task_id.0, limit.0, offset.0),
                &[],
                Some(Duration::from_secs(10)),
                || get_task_leaderboard(&db, &self.state.services, task_id.0, limit.0, offset.0),
            )
            .await??;
        GetTaskLeaderboard::ok(leaderboard.into())
    }

    /// Return the rank of a user on the leaderboard of a task.
    #[oai(path = "/leaderboard/by-task/:task_id/:user_id", method = "get")]
    async fn get_task_leaderboard_user(
        &self,
        task_id: Path<Uuid>,
        user_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetTaskLeaderboardUser::Response<VerifiedUserAuth> {
        match published::use_shared(
            &self.state.services,
            self.config.challenges.profile_publications_enabled,
        )
        .await
        {
            Ok(true) => {
                if !auth.0.email_verified {
                    return published::Error::Unverified.response();
                }
                return match published::rank(
                    &db,
                    &self.state.services,
                    Ranking::Task(task_id.0),
                    user_id.0,
                    &auth.0,
                )
                .await
                {
                    Ok(value) => published::no_store(GetTaskLeaderboardUser::ok(
                        RankResponse::Published(value),
                    )),
                    Err(error) => error.response(),
                };
            }
            Err(error) => return error.response(),
            Ok(false) => {}
        }
        if !is_rank_visible(&self.state.services, &auth.0, user_id.0).await? {
            return GetTaskLeaderboardUser::forbidden();
        }
        let rank = self
            .cache
            .cached_result(
                key!(task_id.0, user_id.0),
                &[&format!("{}", user_id.0)],
                Some(Duration::from_secs(10)),
                || get_task_leaderboard_user(&db, task_id.0, user_id.0),
            )
            .await??;
        GetTaskLeaderboardUser::ok(rank.into())
    }

    /// Return the leaderboard of a programming language.
    ///
    /// See the global leaderboard for publication scope, epoch pagination
    /// and the unchanged legacy treatment of omitted users.
    #[oai(path = "/leaderboard/by-language/:language", method = "get")]
    async fn get_language_leaderboard(
        &self,
        language: Path<String>,
        #[oai(validator(maximum(value = "100")))] limit: Query<u64>,
        offset: Query<u64>,
        publication_epoch: Query<Option<Uuid>>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetLanguageLeaderboard::Response<VerifiedUserAuth> {
        match published::use_shared(
            &self.state.services,
            self.config.challenges.profile_publications_enabled,
        )
        .await
        {
            Ok(true) => {
                if !auth.0.email_verified {
                    return published::Error::Unverified.response();
                }
                return match published::leaderboard(
                    &db,
                    &self.state.services,
                    &self.cache,
                    Ranking::Language(language.0.clone()),
                    limit.0,
                    offset.0,
                    publication_epoch.0,
                )
                .await
                {
                    Ok(value) => published::no_store(GetLanguageLeaderboard::ok(
                        LeaderboardResponse::Published(value),
                    )),
                    Err(error) => error.response(),
                };
            }
            Err(error) => return error.response(),
            Ok(false) => {}
        }
        let leaderboard = self
            .cache
            .cached_result(
                key!(&language.0, limit.0, offset.0),
                &[],
                Some(Duration::from_secs(10)),
                || {
                    get_language_leaderboard(
                        &db,
                        &self.state.services,
                        &language.0,
                        limit.0,
                        offset.0,
                    )
                },
            )
            .await??;
        GetLanguageLeaderboard::ok(leaderboard.into())
    }

    /// Return the rank of a user on the leaderboard of a programming language.
    #[oai(path = "/leaderboard/by-language/:language/:user_id", method = "get")]
    async fn get_language_leaderboard_user(
        &self,
        language: Path<String>,
        user_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetLanguageLeaderboardUser::Response<VerifiedUserAuth> {
        match published::use_shared(
            &self.state.services,
            self.config.challenges.profile_publications_enabled,
        )
        .await
        {
            Ok(true) => {
                if !auth.0.email_verified {
                    return published::Error::Unverified.response();
                }
                return match published::rank(
                    &db,
                    &self.state.services,
                    Ranking::Language(language.0.clone()),
                    user_id.0,
                    &auth.0,
                )
                .await
                {
                    Ok(value) => published::no_store(GetLanguageLeaderboardUser::ok(
                        RankResponse::Published(value),
                    )),
                    Err(error) => error.response(),
                };
            }
            Err(error) => return error.response(),
            Ok(false) => {}
        }
        if !is_rank_visible(&self.state.services, &auth.0, user_id.0).await? {
            return GetLanguageLeaderboardUser::forbidden();
        }
        let rank = self
            .cache
            .cached_result(
                key!(&language.0, user_id.0),
                &[&format!("{}", user_id.0)],
                Some(Duration::from_secs(10)),
                || get_language_leaderboard_user(&db, &language.0, user_id.0),
            )
            .await??;
        GetLanguageLeaderboardUser::ok(rank.into())
    }
}

response!(GetLeaderboard = {
    Ok(200) => LeaderboardResponse,
    Conflict(409, error),
    Unavailable(503, error),
});

response!(GetLeaderboardUser = {
    Ok(200) => RankResponse,
    NotFound(404, error),
    Unavailable(503, error),
    /// The user asked not to be listed on the leaderboards.
    Forbidden(403, error),
});

response!(GetTaskLeaderboard = {
    Ok(200) => LeaderboardResponse,
    Conflict(409, error),
    Unavailable(503, error),
});

response!(GetTaskLeaderboardUser = {
    Ok(200) => RankResponse,
    NotFound(404, error),
    Unavailable(503, error),
    /// The user asked not to be listed on the leaderboards.
    Forbidden(403, error),
});

response!(GetLanguageLeaderboard = {
    Ok(200) => LeaderboardResponse,
    Conflict(409, error),
    Unavailable(503, error),
});

response!(GetLanguageLeaderboardUser = {
    Ok(200) => RankResponse,
    NotFound(404, error),
    Unavailable(503, error),
    /// The user asked not to be listed on the leaderboards.
    Forbidden(403, error),
});

#[cfg(test)]
mod tests {
    use poem_openapi::registry::MetaParamIn;

    use super::*;

    /// Every variable of a route has to be read from the path.
    ///
    /// A variable that is declared in the path but bound as a [`Query`]
    /// parameter is still routed, but the value in the path is ignored and the
    /// request is answered `422` unless the caller repeats the value in the
    /// query string. `GET /leaderboard/{user_id}` did that, which made the
    /// endpoint unreachable in its documented shape.
    #[test]
    fn every_path_variable_is_read_from_the_path() {
        let paths = <LeaderboardEndpoints as OpenApi>::meta()
            .into_iter()
            .flat_map(|api| api.paths);

        let mut checked = 0;
        for path in paths {
            let variables = path
                .path
                .split('/')
                .filter_map(|segment| segment.strip_prefix('{')?.strip_suffix('}'));
            for variable in variables {
                for operation in &path.operations {
                    let param = operation
                        .params
                        .iter()
                        .find(|param| param.name == variable)
                        .unwrap_or_else(|| {
                            panic!("{} does not declare a parameter {variable}", path.path)
                        });
                    assert_eq!(
                        param.in_type,
                        MetaParamIn::Path,
                        "{} does not read {variable} from the path",
                        path.path
                    );
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 7);
    }
}
