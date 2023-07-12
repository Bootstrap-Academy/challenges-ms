use std::{collections::HashMap, sync::Arc};

use chrono::Utc;
use entity::{
    challenges_coding_challenge_result, challenges_coding_challenge_submissions,
    challenges_coding_challenges, challenges_subtasks, challenges_user_subtasks,
    sea_orm_active_enums::ChallengesVerdict,
};
use fnct::{format::JsonFormatter, key};
use key_rwlock::KeyRwLock;
use lib::{auth::VerifiedUserAuth, Cache, SharedState};
use poem::web::Data;
use poem_ext::{db::DbTxn, response, responses::ErrorResponse};
use poem_openapi::{param::Path, payload::Json, OpenApi};
use sandkasten_client::{schemas::environments::Environment, SandkastenClient};
use schemas::challenges::coding_challenges::{Submission, SubmissionContent};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, DbErr, EntityTrait, ModelTrait,
    QueryFilter, Set, TransactionTrait, Unchanged,
};
use thiserror::Error;
use tokio::sync::Semaphore;
use tracing::{debug, error, trace};
use uuid::Uuid;

use super::{check_challenge, CheckChallenge, CheckError, CheckTestcaseError};
use crate::{
    endpoints::Tags,
    services::{
        judge::{self, Judge},
        subtasks::{
            get_subtask, get_user_subtask, send_task_rewards, update_user_subtask,
            SendTaskRewardsError, UserSubtaskExt,
        },
    },
};

pub struct Api {
    pub state: Arc<SharedState>,
    pub sandkasten: SandkastenClient,
    pub judge_cache: Cache<JsonFormatter>,
    pub judge_lock: Arc<Semaphore>,
    pub reward_lock: Arc<KeyRwLock<(Uuid, Uuid)>>,
}

#[OpenApi(tag = "Tags::CodingChallenges")]
impl Api {
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
        let Some((cc, subtask)) =
            get_subtask::<challenges_coding_challenges::Entity>(&db, task_id.0, subtask_id.0)
                .await?
        else {
            return ListSubmissions::subtask_not_found();
        };
        if !auth.0.admin && auth.0.id != subtask.creator && !subtask.enabled {
            return ListSubmissions::subtask_not_found();
        }

        ListSubmissions::ok(
            cc.find_related(challenges_coding_challenge_submissions::Entity)
                .filter(challenges_coding_challenge_submissions::Column::Creator.eq(auth.0.id))
                .find_also_related(challenges_coding_challenge_result::Entity)
                .all(&***db)
                .await?
                .into_iter()
                .map(|(submission, result)| Submission::from(submission, result.map(Into::into)))
                .collect(),
        )
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
        if !auth.0.admin && auth.0.id != subtask.creator && !subtask.enabled {
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
        if !auth.0.admin && auth.0.id != subtask.creator && !subtask.enabled {
            return CreateSubmission::subtask_not_found();
        }

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;
        if !user_subtask.check_access(&auth.0, &subtask) {
            return CreateSubmission::no_access();
        }

        if !self
            .get_environments()
            .await?
            .contains_key(&data.0.environment)
        {
            return CreateSubmission::environment_not_found();
        }

        let submission = challenges_coding_challenge_submissions::ActiveModel {
            id: Set(Uuid::new_v4()),
            subtask_id: Set(cc.subtask_id),
            creator: Set(auth.0.id),
            creation_timestamp: Set(Utc::now().naive_utc()),
            environment: Set(data.0.environment),
            code: Set(data.0.code),
        }
        .insert(&***db)
        .await?;

        tokio::spawn({
            let judge_lock = Arc::clone(&self.judge_lock);
            let submission = submission.clone();
            let db = self.state.db.clone();
            let sandkasten = self.sandkasten.clone();
            let cache = self.judge_cache.clone();
            let reward_lock = Arc::clone(&self.reward_lock);
            let state = Arc::clone(&self.state);
            async move {
                let _guard = judge_lock.acquire().await;
                let submission_id = submission.id;
                let db = match db.begin().await {
                    Ok(x) => x,
                    Err(err) => {
                        error!(
                            "judge task for {submission_id} failed to start db transaction: {err}",
                        );
                        return;
                    }
                };
                let judge = Judge {
                    sandkasten: &sandkasten,
                    evaluator: &cc.evaluator,
                    cache: &cache,
                };
                if let Err(err) = judge_submission(JudgeSubmission {
                    db: &db,
                    subtask: &subtask,
                    challenge: &cc,
                    submission,
                    judge,
                    reward_lock,
                    state,
                    user_subtask,
                })
                .await
                {
                    error!("judge task for {submission_id} failed: {err}");
                    db.rollback().await.ok();
                } else if let Err(err) = db.commit().await {
                    error!("judge task for {submission_id} failed to commit db transaction: {err}");
                }
            }
        });

        CreateSubmission::ok(Submission::from(submission, None))
    }
}

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
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The solution environment does not exist.
    EnvironmentNotFound(404, error),
    /// The user has not unlocked this question.
    NoAccess(403, error),
});

struct JudgeSubmission<'a, 'b> {
    db: &'a DatabaseTransaction,
    subtask: &'a challenges_subtasks::Model,
    challenge: &'a challenges_coding_challenges::Model,
    submission: challenges_coding_challenge_submissions::Model,
    judge: Judge<'b>,
    reward_lock: Arc<KeyRwLock<(Uuid, Uuid)>>,
    state: Arc<SharedState>,
    user_subtask: Option<challenges_user_subtasks::Model>,
}

async fn judge_submission(
    JudgeSubmission {
        db,
        subtask,
        challenge,
        submission,
        judge,
        reward_lock,
        state,
        user_subtask,
    }: JudgeSubmission<'_, '_>,
) -> Result<(), JudgeSubmissionError> {
    debug!("judging submission {}", submission.id);
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
    trace!("judge result for {}: {result:?}", submission.id);
    match result {
        Ok(()) => {
            {
                let _guard = reward_lock
                    .write((submission.subtask_id, submission.creator))
                    .await;
                let solved_previously = challenge
                    .find_related(challenges_coding_challenge_submissions::Entity)
                    .find_also_related(challenges_coding_challenge_result::Entity)
                    .filter(
                        challenges_coding_challenge_submissions::Column::Creator
                            .eq(submission.creator),
                    )
                    .filter(
                        challenges_coding_challenge_result::Column::Verdict
                            .eq(ChallengesVerdict::Ok),
                    )
                    .one(db)
                    .await?
                    .is_some();
                if !solved_previously {
                    update_user_subtask(
                        db,
                        user_subtask.as_ref(),
                        challenges_user_subtasks::ActiveModel {
                            user_id: Set(submission.creator),
                            subtask_id: Set(subtask.id),
                            unlocked_timestamp: user_subtask
                                .as_ref()
                                .and_then(|x| x.unlocked_timestamp)
                                .map(|x| Unchanged(Some(x)))
                                .unwrap_or(Set(Some(submission.creation_timestamp))),
                            solved_timestamp: Set(Some(submission.creation_timestamp)),
                            ..Default::default()
                        },
                    )
                    .await?;

                    if submission.creator != subtask.creator {
                        send_task_rewards(&state.services, db, submission.creator, subtask).await?;
                    }
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
    Judge(#[from] judge::Error),
    #[error("database error: {0}")]
    Db(#[from] DbErr),
    #[error("check error: {0:?}")]
    Check(Box<CheckError>),
    #[error("could not send task rewards: {0}")]
    TaskRewards(#[from] SendTaskRewardsError),
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
