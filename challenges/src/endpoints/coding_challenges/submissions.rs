use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::Context;
use chrono::Utc;
use entity::{
    challenges_coding_challenge_result, challenges_coding_challenge_submissions,
    challenges_coding_challenges, challenges_subtasks, challenges_user_subtasks,
    sea_orm_active_enums::ChallengesVerdict,
};
use fnct::{format::JsonFormatter, key};
use key_rwlock::KeyRwLock;
use lib::{
    auth::{AdminAuth, VerifiedUserAuth},
    config::Config,
    Cache, SharedState,
};
use poem::web::Data;
use poem_ext::{db::DbTxn, response, responses::ErrorResponse};
use poem_openapi::{param::Path, payload::Json, OpenApi};
use sandkasten_client::{schemas::environments::Environment, SandkastenClient};
use schemas::challenges::coding_challenges::{QueueStatus, Submission, SubmissionContent};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, DbErr, EntityTrait, ModelTrait,
    QueryFilter, QueryOrder, Set, TransactionTrait,
};
use thiserror::Error;
use tokio::task::JoinSet;
use tracing::{debug, error, trace};
use uuid::Uuid;

use super::{check_challenge, CheckChallenge, CheckError, CheckTestcaseError};
use crate::{
    endpoints::Tags,
    services::{
        coding_execution::{self as queue, Claim},
        judge::{self, Judge},
        subtasks::{
            get_subtask, get_user_subtask, send_task_rewards, update_user_subtask,
            SendTaskRewardsError, UserSubtaskExt,
        },
    },
};

pub struct Api {
    pub state: Arc<SharedState>,
    pub config: Arc<Config>,
    pub sandkasten: SandkastenClient,
    pub judge_cache: Cache<JsonFormatter>,
}

#[OpenApi(tag = "Tags::CodingChallenges")]
impl Api {
    /// Return the current judge queue status.
    #[oai(path = "/coding_challenges/queue", method = "get")]
    async fn get_queue_status(&self, _auth: AdminAuth) -> GetQueueStatus::Response<AdminAuth> {
        GetQueueStatus::ok(queue::status(&self.state.db).await?)
    }

    /// List all submissions of a coding challenge.
    #[oai(
        path = "/tasks/:task_id/coding_challenges/:subtask_id/submissions",
        method = "get"
    )]
    async fn list_submission(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> ListSubmissions::Response<VerifiedUserAuth> {
        self.list_submission_result(task_id, subtask_id, db, auth, true)
            .await
    }

    /// Get a submission of a coding challenge by id.
    #[oai(
        path = "/tasks/:task_id/coding_challenges/:subtask_id/submissions/:submission_id",
        method = "get"
    )]
    async fn get_submission(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        submission_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetSubmission::Response<VerifiedUserAuth> {
        let Some((cc, subtask)) =
            get_subtask::<challenges_coding_challenges::Entity>(&db, task_id.0, subtask_id.0)
                .await?
        else {
            return GetSubmission::submission_not_found();
        };
        if !auth.0.admin
            && (subtask.moderation_removed || (auth.0.id != subtask.creator && !subtask.enabled))
        {
            return GetSubmission::submission_not_found();
        }

        let Some(submission) =
            challenges_coding_challenge_submissions::Entity::find_by_id(submission_id.0)
                .filter(
                    challenges_coding_challenge_submissions::Column::SubtaskId.eq(cc.subtask_id),
                )
                .filter(challenges_coding_challenge_submissions::Column::Creator.eq(auth.0.id))
                .one(&***db)
                .await?
        else {
            return GetSubmission::submission_not_found();
        };

        GetSubmission::ok(SubmissionContent {
            environment: submission.environment,
            code: submission.code,
        })
    }

    /// Create a submission for a coding challenge.
    #[oai(
        path = "/tasks/:task_id/coding_challenges/:subtask_id/submissions",
        method = "post"
    )]
    async fn create_submission(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        data: Json<SubmissionContent>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> CreateSubmission::Response<VerifiedUserAuth> {
        let Some((cc, subtask)) =
            get_subtask::<challenges_coding_challenges::Entity>(&db, task_id.0, subtask_id.0)
                .await?
        else {
            return CreateSubmission::subtask_not_found();
        };
        if !auth.0.admin
            && (subtask.moderation_removed || (auth.0.id != subtask.creator && !subtask.enabled))
        {
            return CreateSubmission::subtask_not_found();
        }

        if !self
            .get_environments()
            .await?
            .contains_key(&data.0.environment)
        {
            return CreateSubmission::environment_not_found();
        }

        crate::services::benefits::lock_attempt(&db, auth.0.id).await?;
        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;

        if let Some(last_attempt) = user_subtask.last_attempt() {
            let time_left = self.config.challenges.coding_challenges.timeout as i64
                - (Utc::now() - last_attempt).num_seconds();
            if time_left > 0 {
                return CreateSubmission::too_many_requests(time_left as u64);
            }
        }

        let Some(heart_exempt) =
            crate::services::hearts::admit(&self.state.services, &auth.0, &subtask).await?
        else {
            return CreateSubmission::not_enough_hearts();
        };

        if !queue::admit(
            &db,
            auth.0.id,
            &self.config.challenges.coding_challenges.execution,
        )
        .await?
        {
            return CreateSubmission::too_many_requests(u64::from(
                self.config
                    .challenges
                    .coding_challenges
                    .execution
                    .retry_seconds,
            ));
        }

        let submission = Arc::new(
            challenges_coding_challenge_submissions::ActiveModel {
                id: Set(Uuid::new_v4()),
                subtask_id: Set(cc.subtask_id),
                creator: Set(auth.0.id),
                creation_timestamp: Set(Utc::now().naive_utc()),
                environment: Set(data.0.environment),
                code: Set(data.0.code),
                charge_on_failure: Set(!heart_exempt),
                ..Default::default()
            }
            .insert(&***db)
            .await?,
        );

        // The worker only sees the row after the request transaction commits.
        let positions = queue::positions(&***db, auth.0.id).await?;
        CreateSubmission::ok(Submission::from(
            &submission,
            None,
            positions.get(&submission.id).copied(),
        ))
    }

    /// Scoped retained learning only; no ordinary session or publication authority.
    #[oai(
        path = "/learning/tasks/:task_id/coding_challenges/:subtask_id/submissions",
        method = "get"
    )]
    #[allow(clippy::too_many_arguments)]
    async fn learning_list_submission(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: lib::auth::LearningAuth,
    ) -> ListSubmissions::Response<VerifiedUserAuth> {
        let user = crate::services::learning::admit(&db, &self.state.services, auth.0).await?;
        // Reuse product behavior after dedicated scoped admission. This local
        // wrapper value does not pass through any ordinary HTTP authenticator.
        self.list_submission_result(task_id, subtask_id, db, VerifiedUserAuth(user), false)
            .await
    }

    /// Scoped retained learning only; no ordinary session or publication authority.
    #[oai(
        path = "/learning/tasks/:task_id/coding_challenges/:subtask_id/submissions/:submission_id",
        method = "get"
    )]
    #[allow(clippy::too_many_arguments)]
    async fn learning_get_submission(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        submission_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: lib::auth::LearningAuth,
    ) -> GetSubmission::Response<VerifiedUserAuth> {
        let user = crate::services::learning::admit(&db, &self.state.services, auth.0).await?;
        // Reuse product behavior after dedicated scoped admission. This local
        // wrapper value does not pass through any ordinary HTTP authenticator.
        self.get_submission(
            task_id,
            subtask_id,
            submission_id,
            db,
            VerifiedUserAuth(user),
        )
        .await
    }
}

response!(GetQueueStatus = {
    Ok(200) => QueueStatus,
});

response!(ListSubmissions = {
    Ok(200) => Vec<Submission>,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
});

response!(GetSubmission = {
    Ok(200) => SubmissionContent,
    /// Submission does not exist.
    SubmissionNotFound(404, error),
});

response!(CreateSubmission = {
    Ok(201) => Submission,
    /// Try again later. `details` contains the number of seconds to wait.
    TooManyRequests(429, error) => u64,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The solution environment does not exist.
    EnvironmentNotFound(404, error),
    /// The user does not have enough hearts to submit a solution and is neither an admin nor the creator of this subtask.
    NotEnoughHearts(403, error),
});

impl Api {
    async fn list_submission_result(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
        settle_hearts: bool,
    ) -> ListSubmissions::Response<VerifiedUserAuth> {
        let Some((cc, subtask)) =
            get_subtask::<challenges_coding_challenges::Entity>(&db, task_id.0, subtask_id.0)
                .await?
        else {
            return ListSubmissions::subtask_not_found();
        };
        if !auth.0.admin
            && (subtask.moderation_removed || (auth.0.id != subtask.creator && !subtask.enabled))
        {
            return ListSubmissions::subtask_not_found();
        }

        if settle_hearts {
            // A visible final verdict must reconcile its committed debit before
            // the frontend performs its normal server-authoritative heart refresh.
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                crate::services::hearts::settle_user(
                    &self.state.db,
                    &self.state.services,
                    auth.0.id,
                ),
            )
            .await;
        }
        let submissions = cc
            .find_related(challenges_coding_challenge_submissions::Entity)
            .filter(challenges_coding_challenge_submissions::Column::Creator.eq(auth.0.id))
            .find_also_related(challenges_coding_challenge_result::Entity)
            .order_by_desc(challenges_coding_challenge_submissions::Column::CreationTimestamp)
            .all(&***db)
            .await?;
        // Read settlement after the verdict snapshot. Its operation commits in
        // the same transaction; a newly visible wrong verdict cannot look free.
        let pending = crate::services::hearts::unsettled_user(&db, auth.0.id).await?;
        let queue_positions = queue::positions(&***db, auth.0.id).await?;
        ListSubmissions::ok(
            submissions
                .into_iter()
                .map(|(submission, result)| {
                    let position = queue_positions.get(&submission.id).copied();
                    let mut response =
                        Submission::from(&submission, result.map(Into::into), position);
                    response.hearts_pending = pending.contains(&submission.id);
                    response
                })
                .collect(),
        )
    }
}

/// Used by both the default combined process and the standalone worker command.
/// JoinSet owns all slot futures, so stopping this worker also stops heartbeats.
pub async fn run_worker(
    state: Arc<SharedState>,
    config: Arc<Config>,
    sandkasten: SandkastenClient,
) -> anyhow::Result<()> {
    let cc_config = &config.challenges.coding_challenges;
    queue::validate(&cc_config.execution, cc_config.max_concurrency)?;
    let owner = Uuid::new_v4();
    let capacity = cc_config.max_concurrency;
    let settings = cc_config.execution.clone();
    queue::advertise(&state.db, owner, capacity, settings.lease_seconds).await?;
    let mut slots = JoinSet::new();
    for _ in 0..capacity {
        let state = state.clone();
        let settings = settings.clone();
        let sandkasten = sandkasten.clone();
        slots.spawn(async move {
            loop {
                match queue::claim(&state.db, owner, settings.lease_seconds).await {
                    Ok(Some(claim)) => {
                        let work = tokio::time::timeout(
                            Duration::from_secs(u64::from(settings.max_execution_seconds)),
                            execute_claim(state.clone(), &sandkasten, claim),
                        );
                        let heartbeat = async {
                            loop {
                                tokio::time::sleep(Duration::from_secs(u64::from(settings.lease_seconds / 3))).await;
                                if !queue::renew(&state.db, claim, settings.lease_seconds).await? {
                                    anyhow::bail!("coding execution lease lost");
                                }
                            }
                            #[allow(unreachable_code)]
                            Ok::<(), anyhow::Error>(())
                        };
                        let result = tokio::select! {
                            result = work => result.context("coding execution timed out").and_then(|result| result),
                            result = heartbeat => result,
                        };
                        if let Err(err) = result {
                            // Dropping work stops local evaluation. A remote request may
                            // already be running; only a current generation can commit.
                            error!(submission = %claim.submission, "coding execution deferred: {err}");
                            if let Err(err) = queue::retry(&state.db, claim, settings.retry_seconds).await {
                                error!(submission = %claim.submission, "could not defer coding execution: {err}");
                            }
                        } else if let Err(err) = crate::services::hearts::settle(
                            &state.db, &state.services, claim.submission,
                        ).await {
                            error!(submission = %claim.submission, "heart settlement remains pending: {err}");
                        }
                    }
                    Ok(None) => tokio::time::sleep(Duration::from_millis(u64::from(settings.poll_milliseconds))).await,
                    Err(err) => {
                        error!("could not claim coding submission: {err}");
                        tokio::time::sleep(Duration::from_secs(u64::from(settings.retry_seconds))).await;
                    }
                }
            }
        });
    }
    let mut heartbeat =
        tokio::time::interval(Duration::from_secs(u64::from(settings.lease_seconds / 3)));
    loop {
        tokio::select! {
            _ = heartbeat.tick() => queue::advertise(&state.db, owner, capacity, settings.lease_seconds).await?,
            ended = slots.join_next() => anyhow::bail!("coding worker slot stopped: {ended:?}"),
        }
    }
}

async fn execute_claim(
    state: Arc<SharedState>,
    sandkasten: &SandkastenClient,
    claim: Claim,
) -> anyhow::Result<()> {
    let submission = challenges_coding_challenge_submissions::Entity::find_by_id(claim.submission)
        .one(&state.db)
        .await?
        .context("coding submission removed")?;
    let subtask = challenges_subtasks::Entity::find_by_id(submission.subtask_id)
        .one(&state.db)
        .await?
        .context("coding subtask removed")?;
    let challenge = challenges_coding_challenges::Entity::find_by_id(submission.subtask_id)
        .one(&state.db)
        .await?
        .context("coding challenge removed")?;
    let cache = state.cache.with_formatter(JsonFormatter);
    let judge = Judge {
        sandkasten,
        evaluator: &challenge.evaluator,
        cache: &cache,
    };
    debug!("judging submission {}", submission.id);
    // Sandbox I/O is deliberately outside the result transaction.
    let result = check_challenge(CheckChallenge {
        judge,
        challenge_id: challenge.subtask_id,
        solution_environment: &submission.environment,
        solution_code: &submission.code,
        time_limit: challenge.time_limit as _,
        memory_limit: challenge.memory_limit as _,
        static_tests: challenge.static_tests as _,
        random_tests: challenge.random_tests as _,
    })
    .await?;
    trace!("judge finished for {}", submission.id);
    record_claimed_judgment(state, claim, &subtask, &submission, result).await
}

async fn record_claimed_judgment(
    state: Arc<SharedState>,
    claim: Claim,
    subtask: &challenges_subtasks::Model,
    submission: &challenges_coding_challenge_submissions::Model,
    result: Result<(), CheckError>,
) -> anyhow::Result<()> {
    let db = state.db.begin().await?;
    crate::services::benefits::lock_attempt(&db, claim.user).await?;
    anyhow::ensure!(
        queue::fence(&db, claim).await?,
        "coding execution lease lost before result commit"
    );
    record_judgment(
        &db,
        subtask,
        submission,
        result,
        Default::default(),
        state.clone(),
    )
    .await?;
    queue::complete(&db, claim).await?;
    db.commit().await?;
    Ok(())
}

async fn record_judgment(
    db: &DatabaseTransaction,
    subtask: &challenges_subtasks::Model,
    submission: &challenges_coding_challenge_submissions::Model,
    result: Result<(), CheckError>,
    reward_lock: Arc<KeyRwLock<(Uuid, Uuid)>>,
    state: Arc<SharedState>,
) -> Result<(), JudgeSubmissionError> {
    // Both success and failure mutate progress. Serialize with erasure and
    // reread after judging, rather than using a pre-queue progress snapshot.
    let _guard = reward_lock
        .write((submission.subtask_id, submission.creator))
        .await;
    crate::services::benefits::lock_attempt(db, submission.creator).await?;
    if challenges_coding_challenge_result::Entity::find_by_id(submission.id)
        .one(db)
        .await?
        .is_some()
    {
        return Ok(());
    }
    let user_subtask = get_user_subtask(db, submission.creator, subtask.id).await?;
    let last_attempt = user_subtask
        .as_ref()
        .and_then(|row| row.last_attempt_timestamp)
        .unwrap_or(submission.creation_timestamp)
        .max(submission.creation_timestamp);
    match result {
        Ok(()) => {
            let solved_previously = user_subtask.is_solved();
            if !solved_previously {
                update_user_subtask(
                    db,
                    user_subtask.as_ref(),
                    challenges_user_subtasks::ActiveModel {
                        user_id: Set(submission.creator),
                        subtask_id: Set(subtask.id),
                        solved_timestamp: Set(Some(submission.creation_timestamp)),
                        last_attempt_timestamp: Set(Some(last_attempt)),
                        attempts: Set(user_subtask.attempts() as i32 + 1),
                        ..Default::default()
                    },
                )
                .await?;

                if submission.creator != subtask.creator {
                    send_task_rewards(&state.services, db, submission.creator, subtask).await?;
                }
            }
            challenges_coding_challenge_result::ActiveModel {
                submission_id: Set(submission.id),
                verdict: Set(ChallengesVerdict::Ok),
                reason: Set(None),
                build_status: Set(None),
                build_stderr: Set(None),
                build_time: Set(None),
                build_memory: Set(None),
                run_status: Set(None),
                run_stderr: Set(None),
                run_time: Set(None),
                run_memory: Set(None),
            }
            .insert(db)
            .await?;
        }
        Err(CheckError::TestcaseFailed(CheckTestcaseError { result, .. })) => {
            let (build_status, build_stderr, build_time, build_memory) = match result.compile {
                Some(x) => (
                    Some(x.status),
                    Some(x.stderr),
                    Some(x.resource_usage.time as _),
                    Some(x.resource_usage.memory as _),
                ),
                None => (None, None, None, None),
            };
            let (run_status, run_stderr, run_time, run_memory) = match result.run {
                Some(x) => (
                    Some(x.status),
                    Some(x.stderr),
                    Some(x.resource_usage.time as _),
                    Some(x.resource_usage.memory as _),
                ),
                None => (None, None, None, None),
            };
            update_user_subtask(
                db,
                user_subtask.as_ref(),
                challenges_user_subtasks::ActiveModel {
                    user_id: Set(submission.creator),
                    subtask_id: Set(subtask.id),
                    last_attempt_timestamp: Set(Some(last_attempt)),
                    attempts: Set(user_subtask.attempts() as i32 + 1),
                    ..Default::default()
                },
            )
            .await?;
            if submission.charge_on_failure && result.verdict != ChallengesVerdict::Ok {
                crate::services::hearts::record(db, submission.id, submission.creator, subtask.id)
                    .await?;
            }
            challenges_coding_challenge_result::ActiveModel {
                submission_id: Set(submission.id),
                verdict: Set(result.verdict),
                reason: Set(result.reason),
                build_status: Set(build_status),
                build_stderr: Set(build_stderr),
                build_time: Set(build_time),
                build_memory: Set(build_memory),
                run_status: Set(run_status),
                run_stderr: Set(run_stderr),
                run_time: Set(run_time),
                run_memory: Set(run_memory),
            }
            .insert(db)
            .await?;
        }
        Err(err) => return Err(JudgeSubmissionError::Check(Box::new(err))),
    }

    Ok(())
}

#[derive(Debug, Error)]
enum JudgeSubmissionError {
    #[error("failed to judge submission: {0}")]
    Judge(Box<judge::Error>),
    #[error("database error: {0}")]
    Db(#[from] DbErr),
    #[error("check error: {0:?}")]
    Check(Box<CheckError>),
    #[error("could not send task rewards: {0}")]
    TaskRewards(#[from] SendTaskRewardsError),
}

impl From<judge::Error> for JudgeSubmissionError {
    fn from(error: judge::Error) -> Self {
        Self::Judge(Box::new(error))
    }
}

impl Api {
    async fn get_environments(&self) -> Result<HashMap<String, Environment>, ErrorResponse> {
        Ok(self
            .judge_cache
            .cached_result(key!(), &[], None, || async {
                self.sandkasten.list_environments().await
            })
            .await??)
    }
}

#[cfg(test)]
mod heart_tests {
    use super::*;
    use crate::endpoints::heart_tests::Fixture;
    use poem::IntoResponse;
    use schemas::challenges::coding_challenges::CheckResult;
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    async fn submission(
        f: &Fixture,
        subtask: Uuid,
        user: Uuid,
        charge: bool,
    ) -> challenges_coding_challenge_submissions::Model {
        challenges_coding_challenge_submissions::ActiveModel {
            id: Set(Uuid::new_v4()),
            subtask_id: Set(subtask),
            creator: Set(user),
            creation_timestamp: Set(Utc::now().naive_utc()),
            environment: Set("python".into()),
            code: Set("synthetic solution".into()),
            charge_on_failure: Set(charge),
            ..Default::default()
        }
        .insert(&f.state.db)
        .await
        .unwrap()
    }

    fn wrong(verdict: ChallengesVerdict) -> CheckError {
        CheckError::TestcaseFailed(CheckTestcaseError {
            seed: "synthetic".into(),
            result: CheckResult {
                verdict,
                reason: None,
                compile: None,
                run: None,
            },
        })
    }

    async fn count(f: &Fixture, table: &str, user: Uuid) -> i64 {
        f.state
            .db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Postgres,
                format!("SELECT count(*) AS n FROM {table} WHERE user_id=$1"),
                [user.into()],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "n")
            .unwrap()
    }

    /// Run alone against a freshly migrated disposable database; this exercises
    /// the global queue, whereas the other fixtures intentionally share history.
    #[tokio::test]
    #[ignore = "requires fresh disposable PostgreSQL and Redis; run this test separately"]
    async fn coding_durable_execution_postgres() {
        let mut f = Fixture::new().await;
        let (_, id) = f.seed("coding_challenge").await;
        let subtask = challenges_subtasks::Entity::find_by_id(id)
            .one(&f.state.db)
            .await
            .unwrap()
            .unwrap();
        let settings = lib::config::CodingExecution {
            max_pending: 1,
            max_pending_per_user: 1,
            ..Default::default()
        };
        let config = Arc::get_mut(&mut f.config).unwrap();
        config.challenges.coding_challenges.execution = settings.clone();
        config.challenges.coding_challenges.timeout = 0;
        let api = Api {
            state: f.state.clone(),
            config: f.config.clone(),
            sandkasten: SandkastenClient::new(
                f.config.challenges.coding_challenges.sandkasten_url.clone(),
            ),
            judge_cache: f.state.cache.with_formatter(JsonFormatter),
        };
        let owners = [Uuid::new_v4(), Uuid::new_v4()];
        assert_eq!(
            queue::status(&f.state.db).await.unwrap().waiting,
            0,
            "this test needs a fresh database"
        );
        queue::advertise(&f.state.db, owners[0], 2, 30)
            .await
            .unwrap();
        queue::advertise(&f.state.db, owners[1], 3, 30)
            .await
            .unwrap();
        assert_eq!(queue::status(&f.state.db).await.unwrap().workers, 5);

        // API processes share the global cap. The rejected transaction must not
        // insert a submission, progress, rewards or a heart debit.
        let users = [Uuid::new_v4(), Uuid::new_v4()];
        let accept = |user| {
            let f = &f;
            let api = &api;
            let task = subtask.task_id;
            async move {
                let tx = Arc::new(f.state.db.begin().await.unwrap());
                let response = api
                    .create_submission(
                        Path(task),
                        Path(id),
                        Json(SubmissionContent {
                            environment: "python".into(),
                            code: "synthetic".into(),
                        }),
                        Data(&tx),
                        VerifiedUserAuth(lib::auth::User {
                            id: user,
                            email_verified: true,
                            admin: false,
                        }),
                    )
                    .await
                    .unwrap()
                    .into_response();
                let status = response.status();
                let body: serde_json::Value = response.into_body().into_json().await.unwrap();
                let result = if status == poem::http::StatusCode::CREATED {
                    let submission_id: Uuid = body["id"].as_str().unwrap().parse().unwrap();
                    Some(
                        challenges_coding_challenge_submissions::Entity::find_by_id(submission_id)
                            .one(&*tx)
                            .await
                            .unwrap()
                            .unwrap(),
                    )
                } else {
                    assert_eq!(status, poem::http::StatusCode::TOO_MANY_REQUESTS);
                    None
                };
                if result.is_some() {
                    // Acceptance has not committed: a different worker sees nothing.
                    assert!(queue::claim(&f.state.db, owners[0], 30)
                        .await
                        .unwrap()
                        .is_none());
                }
                Arc::try_unwrap(tx).unwrap().commit().await.unwrap();
                result
            }
        };
        let (a, b) = tokio::join!(accept(users[0]), accept(users[1]));
        assert_ne!(a.is_some(), b.is_some());
        let submitted = a.or(b).unwrap();
        for user in users {
            assert_eq!(count(&f, "challenge_heart_operations", user).await, 0);
            assert_eq!(count(&f, "challenge_benefit_earnings", user).await, 0);
            assert_eq!(count(&f, "challenges_user_subtasks", user).await, 0);
        }
        assert_eq!(queue::status(&f.state.db).await.unwrap().waiting, 1);
        assert_eq!(
            queue::positions(&f.state.db, submitted.creator)
                .await
                .unwrap()[&submitted.id],
            1
        );
        let (a, b) = tokio::join!(
            queue::claim(&f.state.db, owners[0], 30),
            queue::claim(&f.state.db, owners[1], 30)
        );
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_ne!(
            a.is_some(),
            b.is_some(),
            "only one concurrent worker may execute this submission"
        );
        let old = a.or(b).unwrap();
        assert_eq!(old.submission, submitted.id);
        assert_eq!(queue::status(&f.state.db).await.unwrap().active, 1);
        assert_eq!(
            queue::positions(&f.state.db, submitted.creator)
                .await
                .unwrap()[&submitted.id],
            0
        );
        assert!(queue::renew(&f.state.db, old, 60).await.unwrap());
        let lease = challenges_coding_challenge_submissions::Entity::find_by_id(submitted.id)
            .one(&f.state.db)
            .await
            .unwrap()
            .unwrap()
            .judge_lease_until
            .unwrap();
        assert!((lease.with_timezone(&Utc) - Utc::now()).num_seconds() >= 58);
        assert!(queue::claim(&f.state.db, owners[1], 30)
            .await
            .unwrap()
            .is_none());

        // A crashed process is replaced. Old renewal, delayed result and retry
        // must all leave the replacement's ownership and side effects intact.
        f.state.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
            "UPDATE challenges_coding_challenge_submissions SET judge_lease_until=now()-interval '1 second' WHERE id=$1", [submitted.id.into()])).await.unwrap();
        assert!(!queue::renew(&f.state.db, old, 30).await.unwrap());
        let current = queue::claim(&f.state.db, owners[1], 30)
            .await
            .unwrap()
            .unwrap();
        assert!(current.generation > old.generation);
        queue::retry(&f.state.db, old, 1000).await.unwrap();
        assert!(record_claimed_judgment(
            f.state.clone(),
            old,
            &subtask,
            &submitted,
            Err(wrong(ChallengesVerdict::WrongAnswer))
        )
        .await
        .is_err());
        assert_eq!(
            count(&f, "challenge_heart_operations", submitted.creator).await,
            0
        );
        assert_eq!(
            count(&f, "challenges_user_subtasks", submitted.creator).await,
            0
        );
        record_claimed_judgment(
            f.state.clone(),
            current,
            &subtask,
            &submitted,
            Err(wrong(ChallengesVerdict::WrongAnswer)),
        )
        .await
        .unwrap();
        assert!(
            record_claimed_judgment(f.state.clone(), current, &subtask, &submitted, Ok(()))
                .await
                .is_err()
        );
        assert_eq!(
            count(&f, "challenge_heart_operations", submitted.creator).await,
            1
        );
        assert_eq!(
            count(&f, "challenge_benefit_earnings", submitted.creator).await,
            0
        );
        let progress = get_user_subtask(&f.state.db.begin().await.unwrap(), submitted.creator, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(progress.attempts, 1);
        assert_eq!(queue::status(&f.state.db).await.unwrap().active, 0);
        assert_eq!(queue::status(&f.state.db).await.unwrap().waiting, 0);
        assert!(
            crate::services::hearts::settle(&f.state.db, &f.state.services, submitted.id)
                .await
                .unwrap()
        );
        assert_eq!(f.shop.lock().unwrap().balances[&submitted.creator], 4);

        // Per-user admission works even with global capacity remaining.
        let user = Uuid::new_v4();
        let retrying = submission(&f, id, user, true).await;
        let settings = lib::config::CodingExecution {
            max_pending: 10,
            max_pending_per_user: 1,
            ..Default::default()
        };
        let tx = f.state.db.begin().await.unwrap();
        assert!(!queue::admit(&tx, user, &settings).await.unwrap());
        assert!(queue::admit(&tx, Uuid::new_v4(), &settings).await.unwrap());
        tx.rollback().await.unwrap();
        let failed = queue::claim(&f.state.db, owners[0], 30)
            .await
            .unwrap()
            .unwrap();
        assert!(record_claimed_judgment(
            f.state.clone(),
            failed,
            &subtask,
            &retrying,
            Err(CheckError::NoExamples)
        )
        .await
        .is_err());
        assert_eq!(count(&f, "challenge_heart_operations", user).await, 0);
        assert_eq!(count(&f, "challenges_user_subtasks", user).await, 0);
        queue::retry(&f.state.db, failed, 10).await.unwrap();
        assert!(queue::claim(&f.state.db, owners[0], 30)
            .await
            .unwrap()
            .is_none());
        f.state.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
            "UPDATE challenges_coding_challenge_submissions SET judge_available_at=now()-interval '1 second' WHERE id=$1", [retrying.id.into()])).await.unwrap();
        let retried = queue::claim(&f.state.db, owners[0], 30)
            .await
            .unwrap()
            .unwrap();
        assert!(retried.generation > failed.generation);
        record_claimed_judgment(f.state.clone(), retried, &subtask, &retrying, Ok(()))
            .await
            .unwrap();
        assert_eq!(count(&f, "challenge_heart_operations", user).await, 0);
        assert_eq!(count(&f, "challenge_benefit_earnings", user).await, 1);
        assert!(queue::claim(&f.state.db, owners[0], 30)
            .await
            .unwrap()
            .is_none());

        f.state.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
            "UPDATE challenge_coding_workers SET lease_until=now()-interval '1 second' WHERE id=$1", [owners[0].into()])).await.unwrap();
        assert_eq!(queue::status(&f.state.db).await.unwrap().workers, 3);
    }

    #[tokio::test]
    #[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
    async fn coding_heart_outcomes_postgres() {
        let f = Fixture::new().await;
        let (_, id) = f.seed("coding_challenge").await;
        let subtask = challenges_subtasks::Entity::find_by_id(id)
            .one(&f.state.db)
            .await
            .unwrap()
            .unwrap();
        let lock = Arc::new(KeyRwLock::default());
        let user = Uuid::new_v4();
        for result in [Ok(()), Ok(())] {
            let submission = submission(&f, id, user, true).await;
            let tx = f.state.db.begin().await.unwrap();
            record_judgment(
                &tx,
                &subtask,
                &submission,
                result,
                lock.clone(),
                f.state.clone(),
            )
            .await
            .unwrap();
            tx.commit().await.unwrap();
        }
        assert_eq!(count(&f, "challenge_heart_operations", user).await, 0);
        assert_eq!(count(&f, "challenge_benefit_earnings", user).await, 1);
        for verdict in [
            ChallengesVerdict::CompilationError,
            ChallengesVerdict::InvalidOutputFormat,
            ChallengesVerdict::MemoryLimitExceeded,
            ChallengesVerdict::NoOutput,
            ChallengesVerdict::PreCheckFailed,
            ChallengesVerdict::RuntimeError,
            ChallengesVerdict::TimeLimitExceeded,
            ChallengesVerdict::WrongAnswer,
        ] {
            let user = Uuid::new_v4();
            let submission = submission(&f, id, user, true).await;
            let tx = f.state.db.begin().await.unwrap();
            record_judgment(
                &tx,
                &subtask,
                &submission,
                Err(wrong(verdict)),
                lock.clone(),
                f.state.clone(),
            )
            .await
            .unwrap();
            assert_eq!(count(&f, "challenge_heart_operations", user).await, 0); // not committed yet
            tx.commit().await.unwrap();
            assert_eq!(count(&f, "challenge_heart_operations", user).await, 1);
            assert!(
                crate::services::hearts::settle(&f.state.db, &f.state.services, submission.id)
                    .await
                    .unwrap()
            );
            assert_eq!(f.shop.lock().unwrap().balances[&user], 4);
            assert_eq!(count(&f, "challenge_benefit_earnings", user).await, 0);
        }
        // Already-paid historical jobs and admission exemptions stay free.
        let exempt = Uuid::new_v4();
        let old = submission(&f, id, exempt, false).await;
        let tx = f.state.db.begin().await.unwrap();
        record_judgment(
            &tx,
            &subtask,
            &old,
            Err(wrong(ChallengesVerdict::WrongAnswer)),
            lock.clone(),
            f.state.clone(),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(count(&f, "challenge_heart_operations", exempt).await, 0);
        // Author/evaluator/environment failures are not learner mistakes.
        let evaluator_failure = serde_json::from_value(serde_json::json!({
            "program_id":Uuid::nil(),"ttl":0,"cached":false,"build":null,
            "run":{"status":1,"stdout":"","stderr":"synthetic evaluator error","resource_usage":{"time":0,"memory":0},"limits":{"cpus":1,"time":1,"memory":16,"tmpfs":0,"filesize":1,"file_descriptors":8,"processes":1,"stdout_max_size":100,"stderr_max_size":100,"network":false}}
        })).unwrap();
        for error in [
            CheckError::NoExamples,
            CheckError::EnvironmentNotFound,
            CheckError::EvaluatorFailed(evaluator_failure),
        ] {
            let user = Uuid::new_v4();
            let submission = submission(&f, id, user, true).await;
            let tx = f.state.db.begin().await.unwrap();
            assert!(record_judgment(
                &tx,
                &subtask,
                &submission,
                Err(error),
                lock.clone(),
                f.state.clone()
            )
            .await
            .is_err());
            tx.rollback().await.unwrap();
            assert_eq!(count(&f, "challenge_heart_operations", user).await, 0);
            assert!(
                challenges_coding_challenge_result::Entity::find_by_id(submission.id)
                    .one(&f.state.db)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[tokio::test]
    #[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
    async fn coding_duplicate_workers_postgres() {
        let f = Fixture::new().await;
        let (_, id) = f.seed("coding_challenge").await;
        let subtask = challenges_subtasks::Entity::find_by_id(id)
            .one(&f.state.db)
            .await
            .unwrap()
            .unwrap();
        let user = Uuid::new_v4();
        let submission = submission(&f, id, user, true).await;
        let worker = || async {
            let tx = f.state.db.begin().await.unwrap();
            // Distinct process-local locks: the database lock is the authority.
            record_judgment(
                &tx,
                &subtask,
                &submission,
                Err(wrong(ChallengesVerdict::WrongAnswer)),
                Arc::new(KeyRwLock::default()),
                f.state.clone(),
            )
            .await
            .unwrap();
            tx.commit().await.unwrap();
        };
        tokio::join!(worker(), worker());
        assert_eq!(count(&f, "challenge_heart_operations", user).await, 1);
        let progress = get_user_subtask(&f.state.db.begin().await.unwrap(), user, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(progress.attempts, 1);
        let api = Api {
            state: f.state.clone(),
            config: f.config.clone(),
            sandkasten: SandkastenClient::new("http://127.0.0.1:9/unused".parse().unwrap()),
            judge_cache: f.state.cache.with_formatter(JsonFormatter),
        };
        let list = |settle| {
            let f = &f;
            let api = &api;
            let task_id = subtask.task_id;
            async move {
                let tx = Arc::new(f.state.db.begin().await.unwrap());
                let response = api
                    .list_submission_result(
                        Path(task_id),
                        Path(id),
                        Data(&tx),
                        VerifiedUserAuth(lib::auth::User {
                            id: user,
                            email_verified: true,
                            admin: false,
                        }),
                        settle,
                    )
                    .await
                    .unwrap()
                    .into_response();
                response
                    .into_body()
                    .into_json::<serde_json::Value>()
                    .await
                    .unwrap()
            }
        };
        let pending = list(false).await;
        assert_eq!(pending[0]["id"], serde_json::json!(submission.id));
        assert_eq!(pending[0]["result"]["verdict"], "WRONG_ANSWER");
        assert_eq!(pending[0]["hearts_pending"], true);
        assert_eq!(f.shop.lock().unwrap().calls, 0); // retained read has no debit authority
        f.shop.lock().unwrap().lose_reply = true;
        assert_eq!(list(true).await[0]["hearts_pending"], true);
        let (a, b) = tokio::join!(
            crate::services::hearts::settle(&f.state.db, &f.state.services, submission.id),
            crate::services::hearts::settle(&f.state.db, &f.state.services, submission.id)
        );
        assert!(a.unwrap() && b.unwrap());
        assert_eq!(f.shop.lock().unwrap().balances[&user], 4);
        assert_eq!(f.shop.lock().unwrap().calls, 2);
        assert_eq!(list(true).await[0]["hearts_pending"], false);
    }
}
