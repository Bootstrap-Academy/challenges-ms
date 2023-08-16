use std::{collections::HashMap, sync::Arc};

use anyhow::{bail, Context};
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
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DatabaseTransaction, DbErr, EntityTrait,
    ModelTrait, QueryFilter, QueryOrder, Set, TransactionTrait,
};
use thiserror::Error;
use tokio::sync::{RwLock, Semaphore};
use tracing::{debug, error, trace};
use uuid::Uuid;

use super::{check_challenge, CheckChallenge, CheckError, CheckTestcaseError};
use crate::{
    endpoints::Tags,
    services::{
        judge::{self, Judge},
        subtasks::{
            deduct_hearts, get_subtask, get_user_subtask, send_task_rewards, update_user_subtask,
            SendTaskRewardsError, UserSubtaskExt,
        },
    },
};

pub struct Api {
    pub state: Arc<SharedState>,
    pub config: Arc<Config>,
    pub sandkasten: SandkastenClient,
    pub judge_cache: Cache<JsonFormatter>,
    pub judge_lock: Arc<Semaphore>,
    pub reward_lock: Arc<KeyRwLock<(Uuid, Uuid)>>,
    pub queue_positions: Arc<RwLock<QueuePositions>>,
}

#[OpenApi(tag = "Tags::CodingChallenges")]
impl Api {
    /// Return the current judge queue status.
    #[oai(path = "/coding_challenges/queue", method = "get")]
    async fn get_queue_status(&self, _auth: AdminAuth) -> GetQueueStatus::Response<AdminAuth> {
        let qp = self.queue_positions.read().await;
        GetQueueStatus::ok(QueueStatus {
            workers: qp.workers(),
            active: qp.active(),
            waiting: qp.waiting(),
        })
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
        let Some((cc, subtask)) =
            get_subtask::<challenges_coding_challenges::Entity>(&db, task_id.0, subtask_id.0)
                .await?
        else {
            return ListSubmissions::subtask_not_found();
        };
        if !auth.0.admin && auth.0.id != subtask.creator && !subtask.enabled {
            return ListSubmissions::subtask_not_found();
        }

        let queue_positions = self.queue_positions.read().await;
        ListSubmissions::ok(
            cc.find_related(challenges_coding_challenge_submissions::Entity)
                .filter(challenges_coding_challenge_submissions::Column::Creator.eq(auth.0.id))
                .find_also_related(challenges_coding_challenge_result::Entity)
                .all(&***db)
                .await?
                .into_iter()
                .map(|(submission, result)| {
                    let position = queue_positions.position(submission.id);
                    Submission::from(&submission, result.map(Into::into), position)
                })
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

        if !self
            .get_environments()
            .await?
            .contains_key(&data.0.environment)
        {
            return CreateSubmission::environment_not_found();
        }

        if !deduct_hearts(&self.state.services, &self.config, &auth.0, &subtask).await? {
            return CreateSubmission::no_access();
        }

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;

        let submission = Arc::new(
            challenges_coding_challenge_submissions::ActiveModel {
                id: Set(Uuid::new_v4()),
                subtask_id: Set(cc.subtask_id),
                creator: Set(auth.0.id),
                creation_timestamp: Set(Utc::now().naive_utc()),
                environment: Set(data.0.environment),
                code: Set(data.0.code),
            }
            .insert(&***db)
            .await?,
        );

        let position = start_judge_submission_task(StartJudgeSubmissionTask {
            submission: Arc::clone(&submission),
            subtask,
            judge_lock: Arc::clone(&self.judge_lock),
            db: self.state.db.clone(),
            sandkasten: self.sandkasten.clone(),
            cache: self.judge_cache.clone(),
            reward_lock: Arc::clone(&self.reward_lock),
            state: Arc::clone(&self.state),
            challenge: Arc::new(cc),
            user_subtask,
            queue_positions: Arc::clone(&self.queue_positions),
        })
        .await;

        CreateSubmission::ok(Submission::from(&submission, None, Some(position)))
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
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The solution environment does not exist.
    EnvironmentNotFound(404, error),
    /// The user does not have enough hearts to submit a solution and is neither an admin nor the creator of this subtask.
    NoAccess(403, error),
});

struct StartJudgeSubmissionTask {
    submission: Arc<challenges_coding_challenge_submissions::Model>,
    subtask: challenges_subtasks::Model,
    judge_lock: Arc<Semaphore>,
    db: DatabaseConnection,
    sandkasten: SandkastenClient,
    cache: Cache<JsonFormatter>,
    reward_lock: Arc<KeyRwLock<(Uuid, Uuid)>>,
    state: Arc<SharedState>,
    challenge: Arc<challenges_coding_challenges::Model>,
    user_subtask: Option<challenges_user_subtasks::Model>,
    queue_positions: Arc<RwLock<QueuePositions>>,
}

async fn start_judge_submission_task(
    StartJudgeSubmissionTask {
        submission,
        judge_lock,
        db,
        sandkasten,
        cache,
        reward_lock,
        state,
        challenge: cc,
        queue_positions,
        subtask,
        user_subtask,
    }: StartJudgeSubmissionTask,
) -> usize {
    let position = queue_positions.write().await.push(submission.id);
    trace!(
        "submission {} enqueued at position {}",
        submission.id,
        position
    );
    tokio::spawn({
        async move {
            let submission_id = submission.id;
            let pop = || async {
                if !queue_positions.write().await.pop(submission_id) {
                    error!("judge task for {submission_id} failed to pop queue position");
                }
            };
            let Ok(_guard) = judge_lock.acquire().await else {
                error!("judge task for {submission_id} failed to acquire lock",);
                // don't pop here since we didn't get the semaphore permit
                return;
            };
            let db = match db.begin().await {
                Ok(x) => x,
                Err(err) => {
                    error!("judge task for {submission_id} failed to start db transaction: {err}",);
                    pop().await;
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
            pop().await;
        }
    });

    position
}

struct JudgeSubmission<'a, 'b> {
    db: &'a DatabaseTransaction,
    subtask: &'a challenges_subtasks::Model,
    challenge: &'a challenges_coding_challenges::Model,
    submission: Arc<challenges_coding_challenge_submissions::Model>,
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
            let _guard = reward_lock
                .write((submission.subtask_id, submission.creator))
                .await;

            let solved_previously = user_subtask.is_solved();
            if !solved_previously {
                update_user_subtask(
                    db,
                    user_subtask.as_ref(),
                    challenges_user_subtasks::ActiveModel {
                        user_id: Set(submission.creator),
                        subtask_id: Set(subtask.id),
                        solved_timestamp: Set(Some(submission.creation_timestamp)),
                        last_attempt_timestamp: Set(Some(submission.creation_timestamp)),
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
                    last_attempt_timestamp: Set(Some(submission.creation_timestamp)),
                    attempts: Set(user_subtask.attempts() as i32 + 1),
                    ..Default::default()
                },
            )
            .await?;
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

    pub async fn setup_api(self) -> anyhow::Result<Self> {
        self.resume_judge()
            .await
            .context("failed to resume judge")?;
        Ok(self)
    }

    pub async fn resume_judge(&self) -> anyhow::Result<()> {
        debug!("resuming judge");
        let db = &self.state.db;

        let subtasks = challenges_subtasks::Entity::find()
            .all(db)
            .await?
            .into_iter()
            .map(|x| (x.id, x))
            .collect::<HashMap<_, _>>();
        let coding_challenges = challenges_coding_challenges::Entity::find()
            .all(db)
            .await?
            .into_iter()
            .map(|x| (x.subtask_id, Arc::new(x)))
            .collect::<HashMap<_, _>>();
        let user_subtasks = challenges_user_subtasks::Entity::find()
            .all(db)
            .await?
            .into_iter()
            .map(|x| ((x.user_id, x.subtask_id), x))
            .collect::<HashMap<_, _>>();
        let submissions = challenges_coding_challenge_submissions::Entity::find()
            .left_join(challenges_coding_challenge_result::Entity)
            .filter(challenges_coding_challenge_result::Column::SubmissionId.is_null())
            .order_by_asc(challenges_coding_challenge_submissions::Column::CreationTimestamp)
            .all(db)
            .await?;

        debug!("found {} submission(s) to judge", submissions.len());
        for submission in submissions {
            let Some(subtask) = subtasks.get(&submission.subtask_id) else {
                bail!(
                    "failed to find subtask {} for submission {}",
                    submission.subtask_id,
                    submission.id
                );
            };
            let Some(challenge) = coding_challenges.get(&submission.subtask_id) else {
                bail!(
                    "failed to find coding challenge {} for submission {}",
                    submission.subtask_id,
                    submission.id
                );
            };
            let user_subtask = user_subtasks.get(&(submission.creator, submission.subtask_id));
            start_judge_submission_task(StartJudgeSubmissionTask {
                submission: Arc::new(submission),
                subtask: subtask.clone(),
                judge_lock: Arc::clone(&self.judge_lock),
                db: db.clone(),
                sandkasten: self.sandkasten.clone(),
                cache: self.judge_cache.clone(),
                reward_lock: Arc::clone(&self.reward_lock),
                state: Arc::clone(&self.state),
                challenge: Arc::clone(challenge),
                user_subtask: user_subtask.cloned(),
                queue_positions: Arc::clone(&self.queue_positions),
            })
            .await;
        }

        Ok(())
    }
}

pub struct QueuePositions {
    workers: usize,
    counter: usize,
    done: usize,
    ids: HashMap<Uuid, usize>,
}

impl QueuePositions {
    pub fn new(workers: usize) -> Self {
        Self {
            workers,
            counter: 0,
            done: 0,
            ids: HashMap::new(),
        }
    }

    pub fn workers(&self) -> usize {
        self.workers
    }

    pub fn active(&self) -> usize {
        self.workers.min(self.counter - self.done)
    }

    pub fn waiting(&self) -> usize {
        self.id_position(self.counter)
    }

    pub fn push(&mut self, key: Uuid) -> usize {
        let id = *self.ids.entry(key).or_insert_with(|| {
            self.counter += 1;
            self.counter
        });
        self.id_position(id)
    }

    pub fn pop(&mut self, key: Uuid) -> bool {
        if !self
            .ids
            .get(&key)
            .is_some_and(|&x| self.id_position(x) == 0)
        {
            return false;
        }

        self.ids.remove(&key);
        self.done += 1;
        true
    }

    pub fn position(&self, key: Uuid) -> Option<usize> {
        let id = *self.ids.get(&key)?;
        Some(self.id_position(id))
    }

    fn id_position(&self, id: usize) -> usize {
        id.saturating_sub(self.workers + self.done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_positions() {
        let mut qp = QueuePositions::new(3);
        assert_eq!(qp.workers(), 3);
        let key = Uuid::from_u128;
        assert_eq!((qp.active(), qp.waiting()), (0, 0));
        qp.push(key(0));
        assert_eq!((qp.active(), qp.waiting()), (1, 0));
        qp.push(key(1));
        assert_eq!((qp.active(), qp.waiting()), (2, 0));
        qp.push(key(2));
        assert_eq!((qp.active(), qp.waiting()), (3, 0));
        qp.push(key(3));
        assert_eq!((qp.active(), qp.waiting()), (3, 1));
        qp.push(key(4));
        assert_eq!((qp.active(), qp.waiting()), (3, 2));
        qp.push(key(5));
        assert_eq!((qp.active(), qp.waiting()), (3, 3));
        assert_eq!(qp.position(key(0)), Some(0));
        assert_eq!(qp.position(key(1)), Some(0));
        assert_eq!(qp.position(key(2)), Some(0));
        assert_eq!(qp.position(key(3)), Some(1));
        assert_eq!(qp.position(key(4)), Some(2));
        assert_eq!(qp.position(key(5)), Some(3));

        // cannot pop pending keys
        assert!(!qp.pop(key(3)));
        assert!(!qp.pop(key(4)));
        assert!(!qp.pop(key(5)));
        assert_eq!((qp.active(), qp.waiting()), (3, 3));

        assert!(qp.pop(key(1)));
        assert_eq!(qp.position(key(0)), Some(0));
        assert_eq!(qp.position(key(1)), None);
        assert_eq!(qp.position(key(2)), Some(0));
        assert_eq!(qp.position(key(3)), Some(0));
        assert_eq!(qp.position(key(4)), Some(1));
        assert_eq!(qp.position(key(5)), Some(2));
        assert_eq!((qp.active(), qp.waiting()), (3, 2));
        assert!(!qp.pop(key(1))); // already popped

        assert!(qp.pop(key(2)));
        assert_eq!(qp.position(key(0)), Some(0));
        assert_eq!(qp.position(key(1)), None);
        assert_eq!(qp.position(key(2)), None);
        assert_eq!(qp.position(key(3)), Some(0));
        assert_eq!(qp.position(key(4)), Some(0));
        assert_eq!(qp.position(key(5)), Some(1));
        assert_eq!((qp.active(), qp.waiting()), (3, 1));

        assert_eq!(qp.push(key(6)), 2);
        assert_eq!((qp.active(), qp.waiting()), (3, 2));
        assert_eq!(qp.push(key(6)), 2); // push is idempotent
        assert_eq!((qp.active(), qp.waiting()), (3, 2));
        assert_eq!(qp.push(key(7)), 3);
        assert_eq!((qp.active(), qp.waiting()), (3, 3));
    }
}
