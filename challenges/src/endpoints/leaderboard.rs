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
use chrono::NaiveDateTime;

use super::Tags;
use crate::services::leaderboard::{
    global::{get_global_leaderboard, get_global_leaderboard_user},
    language::{get_language_leaderboard, get_language_leaderboard_user},
    task::{get_task_leaderboard, get_task_leaderboard_user},
};

pub struct LeaderboardEndpoints {
    pub state: Arc<SharedState>,
    pub cache: Cache<JsonFormatter>,
}

fn get_current_quarter_range() -> (NaiveDateTime, NaiveDateTime) {
    let now = Local::now();
    let year = now.year();
    let quarter = (now.month() - 1) / 3;
    
    let start_month = quarter * 3 + 1;
    let end_month = start_month + 3;
    
    let start = NaiveDateTime::new(
        NaiveDate::from_ymd_opt(year, start_month, 1).unwrap(),
        NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
    );
    
    let end = if end_month > 12 {
        NaiveDateTime::new(
            NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap(),
            NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
        )
    } else {
        NaiveDateTime::new(
            NaiveDate::from_ymd_opt(year, end_month, 1).unwrap(),
            NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
        )
    };
    
    (start, end)
}

#[OpenApi(tag = "Tags::Leaderboard")]
impl LeaderboardEndpoints {
    #[oai(path = "/leaderboard", method = "get")]
    async fn get_leaderboard(
        &self,
        #[oai(validator(maximum(value = "100")))] limit: Query<u64>,
        offset: Query<u64>,
        current_quarter: Query<Option<bool>>,
        _auth: VerifiedUserAuth,
    ) -> GetLeaderboard::Response<VerifiedUserAuth> {
        let date_range = if current_quarter.0.unwrap_or(false) {
            Some(get_current_quarter_range())
        } else {
            None
        };
        
        GetLeaderboard::ok(
            get_global_leaderboard(&self.state.services, limit.0, offset.0, date_range).await?
        )
    }

    #[oai(path = "/leaderboard/:user_id", method = "get")]
    async fn get_leaderboard_user(
        &self,
        user_id: Query<Uuid>,
        current_quarter: Query<Option<bool>>,
        _auth: VerifiedUserAuth,
    ) -> GetLeaderboardUser::Response<VerifiedUserAuth> {
        let date_range = if current_quarter.0.unwrap_or(false) {
            Some(get_current_quarter_range())
        } else {
            None
        };
        
        GetLeaderboardUser::ok(
            get_global_leaderboard_user(&self.state.services, user_id.0, date_range).await?
        )
    }

    #[oai(path = "/leaderboard/by-task/:task_id", method = "get")]
    async fn get_task_leaderboard(
        &self,
        task_id: Path<Uuid>,
        #[oai(validator(maximum(value = "100")))] limit: Query<u64>,
        offset: Query<u64>,
        current_quarter: Query<Option<bool>>,
        db: Data<&DbTxn>,
        _auth: VerifiedUserAuth,
    ) -> GetTaskLeaderboard::Response<VerifiedUserAuth> {
        let date_range = if current_quarter.0.unwrap_or(false) {
            Some(get_current_quarter_range())
        } else {
            None
        };

        let leaderboard = self
            .cache
            .cached_result(
                key!(task_id.0, limit.0, offset.0, current_quarter.0),
                &[],
                Some(Duration::from_secs(10)),
                || get_task_leaderboard(&db, &self.state.services, task_id.0, limit.0, offset.0, date_range),
            )
            .await??;
        GetTaskLeaderboard::ok(leaderboard)
    }

    #[oai(path = "/leaderboard/by-task/:task_id/:user_id", method = "get")]
    async fn get_task_leaderboard_user(
        &self,
        task_id: Path<Uuid>,
        user_id: Path<Uuid>,
        current_quarter: Query<Option<bool>>,
        db: Data<&DbTxn>,
        _auth: VerifiedUserAuth,
    ) -> GetTaskLeaderboardUser::Response<VerifiedUserAuth> {
        let date_range = if current_quarter.0.unwrap_or(false) {
            Some(get_current_quarter_range())
        } else {
            None
        };

        let rank = self
            .cache
            .cached_result(
                key!(task_id.0, user_id.0, current_quarter.0),
                &[],
                Some(Duration::from_secs(10)),
                || get_task_leaderboard_user(&db, task_id.0, user_id.0, date_range),
            )
            .await??;
        GetTaskLeaderboardUser::ok(rank)
    }

    #[oai(path = "/leaderboard/by-language/:language", method = "get")]
    async fn get_language_leaderboard(
        &self,
        language: Path<String>,
        #[oai(validator(maximum(value = "100")))] limit: Query<u64>,
        offset: Query<u64>,
        current_quarter: Query<Option<bool>>,
        db: Data<&DbTxn>,
        _auth: VerifiedUserAuth,
    ) -> GetLanguageLeaderboard::Response<VerifiedUserAuth> {
        let date_range = if current_quarter.0.unwrap_or(false) {
            Some(get_current_quarter_range())
        } else {
            None
        };

        let leaderboard = self
            .cache
            .cached_result(
                key!(&language.0, limit.0, offset.0, current_quarter.0),
                &[],
                Some(Duration::from_secs(10)),
                || {
                    get_language_leaderboard(
                        &db,
                        &self.state.services,
                        &language.0,
                        limit.0,
                        offset.0, 
                        date_range
                    )
                },
            )
            .await??;
        GetLanguageLeaderboard::ok(leaderboard)
    }

    #[oai(path = "/leaderboard/by-language/:language/:user_id", method = "get")]
    async fn get_language_leaderboard_user(
        &self,
        language: Path<String>,
        user_id: Path<Uuid>,
        current_quarter: Query<Option<bool>>,
        db: Data<&DbTxn>,
        _auth: VerifiedUserAuth,
    ) -> GetLanguageLeaderboardUser::Response<VerifiedUserAuth> {
        let date_range = if current_quarter.0.unwrap_or(false) {
            Some(get_current_quarter_range())
        } else {
            None
        };

        let rank = self
            .cache
            .cached_result(
                key!(&language.0, user_id.0, current_quarter.0),
                &[],
                Some(Duration::from_secs(10)),
                || get_language_leaderboard_user(&db, &language.0, user_id.0, date_range),
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
});

response!(GetTaskLeaderboard = {
    Ok(200) => Leaderboard,
});

response!(GetTaskLeaderboardUser = {
    Ok(200) => Rank,
});

response!(GetLanguageLeaderboard = {
    Ok(200) => Leaderboard,
});

response!(GetLanguageLeaderboardUser = {
    Ok(200) => Rank,
});
