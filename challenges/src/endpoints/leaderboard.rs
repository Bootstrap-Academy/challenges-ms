use std::{sync::Arc, time::Duration};

use fnct::{format::JsonFormatter, key};
use lib::{auth::VerifiedUserAuth, Cache, SharedState};
use poem::web::Data;
use poem_ext::{db::DbTxn, response};
use poem_openapi::{
    param::{Path, Query},
    OpenApi,
};
use schemas::challenges::leaderboard::{Leaderboard, Rank};
use uuid::Uuid;

use super::Tags;
use crate::services::leaderboard::{
    global::{get_global_leaderboard, get_global_leaderboard_user},
    is_rank_visible,
    language::{get_language_leaderboard, get_language_leaderboard_user},
    task::{get_task_leaderboard, get_task_leaderboard_user},
};

pub struct LeaderboardEndpoints {
    pub state: Arc<SharedState>,
    pub cache: Cache<JsonFormatter>,
}

#[OpenApi(tag = "Tags::Leaderboard")]
impl LeaderboardEndpoints {
    /// Return the global leaderboard.
    ///
    /// Users who asked not to be listed are omitted, so a page may contain
    /// fewer entries than requested and the ranks of the remaining users may
    /// have gaps. `total` counts all positions on the leaderboard, including
    /// the omitted ones, so that pagination by `offset` stays exact.
    #[oai(path = "/leaderboard", method = "get")]
    async fn get_leaderboard(
        &self,
        #[oai(validator(maximum(value = "100")))] limit: Query<u64>,
        offset: Query<u64>,
        _auth: VerifiedUserAuth,
    ) -> GetLeaderboard::Response<VerifiedUserAuth> {
        GetLeaderboard::ok(get_global_leaderboard(&self.state.services, limit.0, offset.0).await?)
    }

    /// Return the rank of a user on the global leaderboard.
    #[oai(path = "/leaderboard/:user_id", method = "get")]
    async fn get_leaderboard_user(
        &self,
        user_id: Path<Uuid>,
        auth: VerifiedUserAuth,
    ) -> GetLeaderboardUser::Response<VerifiedUserAuth> {
        if !is_rank_visible(&self.state.services, &auth.0, user_id.0).await? {
            return GetLeaderboardUser::forbidden();
        }
        GetLeaderboardUser::ok(get_global_leaderboard_user(&self.state.services, user_id.0).await?)
    }

    /// Return the leaderboard of a task.
    ///
    /// Users who asked not to be listed are omitted; see the global
    /// leaderboard for what that means for `total` and the ranks.
    #[oai(path = "/leaderboard/by-task/:task_id", method = "get")]
    async fn get_task_leaderboard(
        &self,
        task_id: Path<Uuid>,
        #[oai(validator(maximum(value = "100")))] limit: Query<u64>,
        offset: Query<u64>,
        db: Data<&DbTxn>,
        _auth: VerifiedUserAuth,
    ) -> GetTaskLeaderboard::Response<VerifiedUserAuth> {
        let leaderboard = self
            .cache
            .cached_result(
                key!(task_id.0, limit.0, offset.0),
                &[],
                Some(Duration::from_secs(10)),
                || get_task_leaderboard(&db, &self.state.services, task_id.0, limit.0, offset.0),
            )
            .await??;
        GetTaskLeaderboard::ok(leaderboard)
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
        GetTaskLeaderboardUser::ok(rank)
    }

    /// Return the leaderboard of a programming language.
    ///
    /// Users who asked not to be listed are omitted; see the global
    /// leaderboard for what that means for `total` and the ranks.
    #[oai(path = "/leaderboard/by-language/:language", method = "get")]
    async fn get_language_leaderboard(
        &self,
        language: Path<String>,
        #[oai(validator(maximum(value = "100")))] limit: Query<u64>,
        offset: Query<u64>,
        db: Data<&DbTxn>,
        _auth: VerifiedUserAuth,
    ) -> GetLanguageLeaderboard::Response<VerifiedUserAuth> {
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
        GetLanguageLeaderboard::ok(leaderboard)
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
        GetLanguageLeaderboardUser::ok(rank)
    }
}

response!(GetLeaderboard = {
    Ok(200) => Leaderboard,
});

response!(GetLeaderboardUser = {
    Ok(200) => Rank,
    /// The user asked not to be listed on the leaderboards.
    Forbidden(403, error),
});

response!(GetTaskLeaderboard = {
    Ok(200) => Leaderboard,
});

response!(GetTaskLeaderboardUser = {
    Ok(200) => Rank,
    /// The user asked not to be listed on the leaderboards.
    Forbidden(403, error),
});

response!(GetLanguageLeaderboard = {
    Ok(200) => Leaderboard,
});

response!(GetLanguageLeaderboardUser = {
    Ok(200) => Rank,
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
