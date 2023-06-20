use std::sync::Arc;

use chrono::{DateTime, Utc};
use entity::{
    challenges_coding_challenges, challenges_subtasks, sea_orm_active_enums::ChallengesBanAction,
};
use fnct::format::JsonFormatter;
use lib::{
    auth::{AdminAuth, VerifiedUserAuth},
    config::Config,
    Cache, SharedState,
};
use poem::web::Data;
use poem_ext::{db::DbTxn, response};
use poem_openapi::{
    param::{Path, Query},
    payload::Json,
    OpenApi,
};
use sandkasten_client::SandkastenClient;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, ModelTrait, QueryFilter, QueryOrder, Set, Unchanged,
};
use tracing::error;
use uuid::Uuid;

use super::{_CheckError, check_challenge, get_challenge, CheckChallenge};
use crate::{
    endpoints::Tags,
    schemas::coding_challenges::{
        CodingChallenge, CodingChallengeSummary, CreateCodingChallengeRequest, Example,
        SubmissionContent, UpdateCodingChallengeRequest,
    },
    services::{
        judge::{self, get_executor_config, Judge},
        subtasks::{
            can_create, get_active_ban, get_user_subtask, get_user_subtasks, ActiveBan,
            UserSubtaskExt,
        },
        tasks::{get_task, get_task_with_specific, Task},
    },
};

pub struct Api {
    pub sandkasten: SandkastenClient,
    pub judge_cache: Cache<JsonFormatter>,
    pub config: Arc<Config>,
    pub state: Arc<SharedState>,
}

#[OpenApi(tag = "Tags::CodingChallenges")]
impl Api {
    /// List all coding challenges in a task.
    #[allow(clippy::too_many_arguments)]
    #[oai(path = "/tasks/:task_id/coding_challenges", method = "get")]
    async fn list_challenges(
        &self,
        task_id: Path<Uuid>,
        /// Whether to search for free coding challenges.
        free: Query<Option<bool>>,
        /// Whether to search for unlocked coding challenges.
        unlocked: Query<Option<bool>>,
        /// Whether to search for solved challenges.
        solved: Query<Option<bool>>,
        /// Whether to search for rated challenges.
        rated: Query<Option<bool>>,
        /// Whether to search for enabled subtasks.
        enabled: Query<Option<bool>>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> ListCodingChallenges::Response<VerifiedUserAuth> {
        let subtasks = get_user_subtasks(&db, auth.0.id).await?;
        ListCodingChallenges::ok(
            challenges_coding_challenges::Entity::find()
                .find_also_related(challenges_subtasks::Entity)
                .filter(challenges_subtasks::Column::TaskId.eq(task_id.0))
                .order_by_asc(challenges_subtasks::Column::CreationTimestamp)
                .all(&***db)
                .await?
                .into_iter()
                .filter_map(|(cc, subtask)| {
                    let subtask = subtask?;
                    let id = subtask.id;
                    let free_ = subtask.fee <= 0;
                    let unlocked_ = subtasks.get(&id).check_access(&auth.0, &subtask);
                    let solved_ = subtasks.get(&id).is_solved();
                    let rated_ = subtasks.get(&id).is_rated();
                    let enabled_ = subtask.enabled;
                    ((auth.0.admin || auth.0.id == subtask.creator || subtask.enabled)
                        && free.unwrap_or(free_) == free_
                        && unlocked.unwrap_or(unlocked_) == unlocked_
                        && solved.unwrap_or(solved_) == solved_
                        && rated.unwrap_or(rated_) == rated_
                        && enabled.unwrap_or(enabled_) == enabled_)
                        .then_some(CodingChallengeSummary::from(
                            cc, subtask, unlocked_, solved_, rated_,
                        ))
                })
                .collect(),
        )
    }

    /// Get a coding challenge by id.
    #[oai(path = "/tasks/:task_id/coding_challenges/:subtask_id", method = "get")]
    async fn get_challenge(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetCodingChallenge::Response<VerifiedUserAuth> {
        let Some((cc, subtask)) = get_challenge(&db, task_id.0, subtask_id.0).await? else {
            return GetCodingChallenge::subtask_not_found();
        };
        if !auth.0.admin && auth.0.id != subtask.creator && !subtask.enabled {
            return GetCodingChallenge::subtask_not_found();
        }

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;
        if !user_subtask.check_access(&auth.0, &subtask) {
            return GetCodingChallenge::no_access();
        }

        GetCodingChallenge::ok(CodingChallenge::from(
            cc,
            subtask,
            user_subtask.is_solved(),
            user_subtask.is_rated(),
        ))
    }

    /// Get the examples of a coding challenge by id.
    #[oai(
        path = "/tasks/:task_id/coding_challenges/:subtask_id/examples",
        method = "get"
    )]
    async fn get_examples(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetExamples::Response<VerifiedUserAuth> {
        let Some((cc, subtask)) = get_challenge(&db, task_id.0, subtask_id.0).await? else {
            return GetExamples::subtask_not_found();
        };
        if !auth.0.admin && auth.0.id != subtask.creator && !subtask.enabled {
            return GetExamples::subtask_not_found();
        }

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;
        if !user_subtask.check_access(&auth.0, &subtask) {
            return GetExamples::no_access();
        }

        let judge = self.get_judge(&cc.evaluator);

        let examples = match judge.examples().await {
            Err(judge::Error::EvaluatorFailed(err) | judge::Error::InvalidOutput(err)) => {
                error!(
                    "evaluator for {} failed to execute: {:?}",
                    subtask_id.0, err
                );
                return GetExamples::evaluator_failed();
            }
            x => x?,
        };
        let mut out = Vec::with_capacity(examples.len());
        for seed in &examples {
            let example = judge
                .get_example_checked(
                    seed,
                    &cc.solution_environment,
                    &cc.solution_code,
                    Some(cc.time_limit as _),
                    Some(cc.memory_limit as _),
                )
                .await?;
            let example = match example {
                Ok(example) => example,
                Err(err) => {
                    error!(
                        "example generation for {} failed on example {}: {:?}",
                        subtask_id.0, seed, err
                    );
                    return GetExamples::example_generation_failed();
                }
            };
            out.push(example);
        }

        GetExamples::ok(out)
    }

    /// Get the evaluator of a coding challenge by id.
    #[oai(
        path = "/tasks/:task_id/coding_challenges/:subtask_id/evaluator",
        method = "get"
    )]
    async fn get_evaluator(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetEvaluator::Response<VerifiedUserAuth> {
        let Some((cc, subtask)) = get_challenge(&db, task_id.0, subtask_id.0).await? else {
            return GetEvaluator::subtask_not_found();
        };

        if auth.0.admin || auth.0.id == subtask.creator {
            GetEvaluator::ok(cc.evaluator)
        } else {
            GetEvaluator::forbidden()
        }
    }

    /// Get the solution of a coding challenge by id.
    #[oai(
        path = "/tasks/:task_id/coding_challenges/:subtask_id/solution",
        method = "get"
    )]
    async fn get_solution(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetSolution::Response<VerifiedUserAuth> {
        let Some((cc, subtask)) = get_challenge(&db, task_id.0, subtask_id.0).await? else {
            return GetSolution::subtask_not_found();
        };

        if auth.0.admin || auth.0.id == subtask.creator {
            GetSolution::ok(SubmissionContent {
                environment: cc.solution_environment,
                code: cc.solution_code,
            })
        } else {
            GetSolution::forbidden()
        }
    }

    /// Create a new coding challenge.
    #[oai(path = "/tasks/:task_id/coding_challenges", method = "post")]
    async fn create_challenge(
        &self,
        task_id: Path<Uuid>,
        data: Json<CreateCodingChallengeRequest>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> CreateCodingChallenge::Response<VerifiedUserAuth> {
        let (task, specific) = match get_task_with_specific(&db, task_id.0).await? {
            Some(task) => task,
            None => return CreateCodingChallenge::task_not_found(),
        };
        if !can_create(&self.state.services, &self.config, &specific, &auth.0).await? {
            return CreateCodingChallenge::forbidden();
        }

        if matches!(specific, Task::CourseTask(_)) && !auth.0.admin {
            if data.0.xp > self.config.challenges.quizzes.max_xp {
                return CreateCodingChallenge::xp_limit_exceeded(
                    self.config.challenges.quizzes.max_xp,
                );
            }
            if data.0.coins > self.config.challenges.quizzes.max_coins {
                return CreateCodingChallenge::coin_limit_exceeded(
                    self.config.challenges.quizzes.max_coins,
                );
            }
            if data.0.fee > self.config.challenges.quizzes.max_fee {
                return CreateCodingChallenge::fee_limit_exceeded(
                    self.config.challenges.quizzes.max_fee,
                );
            }
        }

        match get_active_ban(&db, &auth.0, ChallengesBanAction::Create).await? {
            ActiveBan::NotBanned => {}
            ActiveBan::Temporary(end) => return CreateCodingChallenge::banned(Some(end)),
            ActiveBan::Permanent => return CreateCodingChallenge::banned(None),
        }

        let config = get_executor_config(&self.judge_cache, &self.sandkasten).await?;
        if data.0.time_limit > (config.run_limits.time - 1) * 1000 {
            return CreateCodingChallenge::time_limit_exceeded((config.run_limits.time - 1) * 1000);
        }
        if data.0.memory_limit > config.run_limits.memory {
            return CreateCodingChallenge::memory_limit_exceeded(config.run_limits.memory);
        }

        let cc_id = Uuid::new_v4();
        if let Err(result) = check_challenge(CheckChallenge {
            judge: self.get_judge(&data.0.evaluator),
            challenge_id: cc_id,
            solution_environment: &data.0.solution_environment,
            solution_code: &data.0.solution_code,
            time_limit: data.0.time_limit,
            memory_limit: data.0.memory_limit,
            static_tests: data.0.static_tests,
            random_tests: data.0.random_tests,
        })
        .await?
        {
            return Ok(_CheckError::Response::from(result).into());
        }

        let subtask = challenges_subtasks::ActiveModel {
            id: Set(cc_id),
            task_id: Set(task.id),
            creator: Set(auth.0.id),
            creation_timestamp: Set(Utc::now().naive_utc()),
            xp: Set(data.0.xp as _),
            coins: Set(data.0.coins as _),
            fee: Set(data.0.fee as _),
            enabled: Set(true),
        }
        .insert(&***db)
        .await?;
        let cc = challenges_coding_challenges::ActiveModel {
            subtask_id: Set(subtask.id),
            time_limit: Set(data.0.time_limit as _),
            memory_limit: Set(data.0.memory_limit as _),
            static_tests: Set(data.0.static_tests as _),
            random_tests: Set(data.0.random_tests as _),
            evaluator: Set(data.0.evaluator),
            description: Set(data.0.description),
            solution_environment: Set(data.0.solution_environment),
            solution_code: Set(data.0.solution_code),
        }
        .insert(&***db)
        .await?;
        CreateCodingChallenge::ok(CodingChallenge::from(cc, subtask, false, false))
    }

    /// Update a coding challenge.
    #[oai(
        path = "/tasks/:task_id/coding_challenges/:subtask_id",
        method = "patch"
    )]
    async fn update_challenge(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        data: Json<UpdateCodingChallengeRequest>,
        db: Data<&DbTxn>,
        auth: AdminAuth,
    ) -> UpdateCodingChallenge::Response<AdminAuth> {
        let Some((cc, subtask)) = get_challenge(&db, task_id.0, subtask_id.0).await? else {
            return UpdateCodingChallenge::subtask_not_found();
        };

        if get_task(&db, *data.0.task_id.get_new(&subtask.task_id))
            .await?
            .is_none()
        {
            return UpdateCodingChallenge::task_not_found();
        }

        let config = get_executor_config(&self.judge_cache, &self.sandkasten).await?;
        if *data.0.time_limit.get_new(&(cc.time_limit as _)) > (config.run_limits.time - 1) * 1000 {
            return UpdateCodingChallenge::time_limit_exceeded((config.run_limits.time - 1) * 1000);
        }
        if *data.0.memory_limit.get_new(&(cc.time_limit as _)) > config.run_limits.memory {
            return UpdateCodingChallenge::memory_limit_exceeded(config.run_limits.memory);
        }

        if let Err(result) = check_challenge(CheckChallenge {
            judge: self.get_judge(data.0.evaluator.get_new(&cc.evaluator)),
            challenge_id: cc.subtask_id,
            solution_environment: data
                .0
                .solution_environment
                .get_new(&cc.solution_environment),
            solution_code: data.0.solution_code.get_new(&cc.solution_code),
            time_limit: *data.0.time_limit.get_new(&(cc.time_limit as _)),
            memory_limit: *data.0.memory_limit.get_new(&(cc.memory_limit as _)),
            static_tests: *data.0.static_tests.get_new(&(cc.static_tests as _)),
            random_tests: *data.0.random_tests.get_new(&(cc.random_tests as _)),
        })
        .await?
        {
            return Ok(_CheckError::Response::from(result).into());
        }

        let cc = challenges_coding_challenges::ActiveModel {
            subtask_id: Unchanged(cc.subtask_id),
            time_limit: data.0.time_limit.map(|x| x as _).update(cc.time_limit),
            memory_limit: data.0.memory_limit.map(|x| x as _).update(cc.memory_limit),
            static_tests: data.0.static_tests.map(|x| x as _).update(cc.static_tests),
            random_tests: data.0.random_tests.map(|x| x as _).update(cc.random_tests),
            evaluator: data.0.evaluator.update(cc.evaluator),
            description: data.0.description.update(cc.description),
            solution_environment: data.0.solution_environment.update(cc.solution_environment),
            solution_code: data.0.solution_code.update(cc.solution_code),
        }
        .update(&***db)
        .await?;

        let subtask = challenges_subtasks::ActiveModel {
            id: Unchanged(subtask.id),
            task_id: data.0.task_id.update(subtask.task_id),
            creator: Unchanged(subtask.creator),
            creation_timestamp: Unchanged(subtask.creation_timestamp),
            xp: data.0.xp.map(|x| x as _).update(subtask.xp),
            coins: data.0.coins.map(|x| x as _).update(subtask.coins),
            fee: data.0.fee.map(|x| x as _).update(subtask.fee),
            enabled: data.0.enabled.update(subtask.enabled),
        }
        .update(&***db)
        .await?;

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;
        UpdateCodingChallenge::ok(CodingChallenge::from(
            cc,
            subtask,
            user_subtask.is_solved(),
            user_subtask.is_rated(),
        ))
    }

    /// Delete a coding challenge.
    #[oai(
        path = "/tasks/:task_id/coding_challenges/:subtask_id",
        method = "delete"
    )]
    async fn delete_challenge(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> DeleteCodingChallenge::Response<AdminAuth> {
        match get_challenge(&db, task_id.0, subtask_id.0).await? {
            Some((_, subtask)) => {
                subtask.delete(&***db).await?;
                DeleteCodingChallenge::ok()
            }
            None => DeleteCodingChallenge::subtask_not_found(),
        }
    }
}

response!(ListCodingChallenges = {
    Ok(200) => Vec<CodingChallengeSummary>,
});

response!(GetCodingChallenge = {
    Ok(200) => CodingChallenge,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user has not unlocked this question.
    NoAccess(403, error),
});

response!(GetExamples = {
    Ok(200) => Vec<Example>,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user has not unlocked this question.
    NoAccess(403, error),
    /// The evaluator failed to execute.
    EvaluatorFailed(400, error),
    /// Failed to generate an example.
    ExampleGenerationFailed(400, error),
});

response!(GetEvaluator = {
    Ok(200) => String,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user is not allowed to request the evaluator of this coding challenge.
    Forbidden(403, error),
});

response!(GetSolution = {
    Ok(200) => SubmissionContent,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user is not allowed to request the solution of this coding challenge.
    Forbidden(403, error),
});

response!(CreateCodingChallenge = {
    Ok(201) => CodingChallenge,
    /// Task does not exist.
    TaskNotFound(404, error),
    /// The user is not allowed to create questions in this task.
    Forbidden(403, error),
    /// The user is currently banned from creating subtasks.
    Banned(403, error) => Option<DateTime<Utc>>,
    /// The max xp limit has been exceeded.
    XpLimitExceeded(403, error) => u64,
    /// The max coin limit has been exceeded.
    CoinLimitExceeded(403, error) => u64,
    /// The max fee limit has been exceeded.
    FeeLimitExceeded(403, error) => u64,
    /// Time limit exceeded
    TimeLimitExceeded(403, error) => u64,
    /// Memory limit exceeded
    MemoryLimitExceeded(403, error) => u64,
    .._CheckError::Response,
});

response!(UpdateCodingChallenge = {
    Ok(200) => CodingChallenge,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// Task does not exist.
    TaskNotFound(404, error),
    /// Time limit exceeded
    TimeLimitExceeded(403, error) => u64,
    /// Memory limit exceeded
    MemoryLimitExceeded(403, error) => u64,
    .._CheckError::Response,
});

response!(DeleteCodingChallenge = {
    Ok(200),
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
});

impl Api {
    fn get_judge<'a>(&'a self, evaluator: &'a str) -> Judge<'a> {
        Judge {
            sandkasten: &self.sandkasten,
            evaluator,
            cache: &self.judge_cache,
        }
    }
}
