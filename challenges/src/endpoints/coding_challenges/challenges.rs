use std::sync::Arc;

use chrono::{DateTime, Utc};
use entity::{challenges_coding_challenges, sea_orm_active_enums::ChallengesSubtaskType};
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
use schemas::challenges::coding_challenges::{
    CodingChallenge, CodingChallengeSummary, CreateCodingChallengeRequest, Example,
    SubmissionContent, UpdateCodingChallengeRequest,
};
use sea_orm::{ActiveModelTrait, Set, Unchanged};
use tracing::error;
use uuid::Uuid;

use super::{_CheckError, check_challenge, CheckChallenge};
use crate::{
    endpoints::Tags,
    services::{
        judge::{self, get_executor_config, Judge},
        subtasks::{
            create_subtask, query_subtask, query_subtask_admin, query_subtasks, update_subtask,
            CreateSubtaskError, QuerySubtaskError, QuerySubtasksFilter, UpdateSubtaskError,
        },
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
        /// Whether to search for subtasks the user has attempted to solve.
        attempted: Query<Option<bool>>,
        /// Whether to search for solved subtasks.
        solved: Query<Option<bool>>,
        /// Whether to search for rated subtasks.
        rated: Query<Option<bool>>,
        /// Whether to search for enabled subtasks.
        enabled: Query<Option<bool>>,
        /// Filter by creator.
        creator: Query<Option<Uuid>>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> ListCodingChallenges::Response<VerifiedUserAuth> {
        ListCodingChallenges::ok(
            query_subtasks::<challenges_coding_challenges::Entity, _>(
                &db,
                &auth.0,
                task_id.0,
                QuerySubtasksFilter {
                    attempted: attempted.0,
                    solved: solved.0,
                    rated: rated.0,
                    enabled: enabled.0,
                    creator: creator.0,
                    ty: None,
                },
                CodingChallengeSummary::from,
            )
            .await?,
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
        match query_subtask::<challenges_coding_challenges::Entity, _>(
            &db,
            &auth.0,
            task_id.0,
            subtask_id.0,
            CodingChallenge::from,
        )
        .await?
        {
            Ok(x) => GetCodingChallenge::ok(x),
            Err(QuerySubtaskError::NotFound) => GetCodingChallenge::subtask_not_found(),
            Err(QuerySubtaskError::NoAccess) => GetCodingChallenge::no_access(),
        }
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
        let cc = match query_subtask::<challenges_coding_challenges::Entity, _>(
            &db,
            &auth.0,
            task_id.0,
            subtask_id.0,
            |cc, _| cc,
        )
        .await?
        {
            Ok(cc) => cc,
            Err(QuerySubtaskError::NotFound) => return GetExamples::subtask_not_found(),
            Err(QuerySubtaskError::NoAccess) => return GetExamples::no_access(),
        };

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
        match query_subtask_admin::<challenges_coding_challenges::Entity, _>(
            &db,
            &auth.0,
            task_id.0,
            subtask_id.0,
            |cc, _| cc,
        )
        .await?
        {
            Ok(cc) => GetEvaluator::ok(cc.evaluator),
            Err(QuerySubtaskError::NotFound) => GetEvaluator::subtask_not_found(),
            Err(QuerySubtaskError::NoAccess) => GetEvaluator::forbidden(),
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
        match query_subtask_admin::<challenges_coding_challenges::Entity, _>(
            &db,
            &auth.0,
            task_id.0,
            subtask_id.0,
            |cc, _| cc,
        )
        .await?
        {
            Ok(cc) => GetSolution::ok(SubmissionContent {
                environment: cc.solution_environment,
                code: cc.solution_code,
            }),
            Err(QuerySubtaskError::NotFound) => GetSolution::subtask_not_found(),
            Err(QuerySubtaskError::NoAccess) => GetSolution::forbidden(),
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
        let subtask = match create_subtask(
            &db,
            &self.state.services,
            &self.config,
            &auth.0,
            task_id.0,
            data.0.subtask,
            ChallengesSubtaskType::CodingChallenge,
        )
        .await?
        {
            Ok(subtask) => subtask,
            Err(CreateSubtaskError::TaskNotFound) => {
                return CreateCodingChallenge::task_not_found()
            }
            Err(CreateSubtaskError::Forbidden) => return CreateCodingChallenge::forbidden(),
            Err(CreateSubtaskError::Banned(until)) => return CreateCodingChallenge::banned(until),
            Err(CreateSubtaskError::XpLimitExceeded(x)) => {
                return CreateCodingChallenge::xp_limit_exceeded(x)
            }
            Err(CreateSubtaskError::CoinLimitExceeded(x)) => {
                return CreateCodingChallenge::coin_limit_exceeded(x)
            }
        };

        let config = get_executor_config(&self.judge_cache, &self.sandkasten).await?;
        if data.0.time_limit > config.time_limit {
            return CreateCodingChallenge::time_limit_exceeded(config.time_limit);
        }
        if data.0.memory_limit > config.memory_limit {
            return CreateCodingChallenge::memory_limit_exceeded(config.memory_limit);
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
        CreateCodingChallenge::ok(CodingChallenge::from(cc, subtask))
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
        let (cc, subtask) = match update_subtask::<challenges_coding_challenges::Entity>(
            &db,
            &auth.0,
            task_id.0,
            subtask_id.0,
            data.0.subtask,
        )
        .await?
        {
            Ok(x) => x,
            Err(UpdateSubtaskError::SubtaskNotFound) => {
                return UpdateCodingChallenge::subtask_not_found()
            }
            Err(UpdateSubtaskError::TaskNotFound) => {
                return UpdateCodingChallenge::task_not_found()
            }
        };

        let config = get_executor_config(&self.judge_cache, &self.sandkasten).await?;
        if *data.0.time_limit.get_new(&(cc.time_limit as _)) > config.time_limit {
            return UpdateCodingChallenge::time_limit_exceeded(config.time_limit);
        }
        if *data.0.memory_limit.get_new(&(cc.memory_limit as _)) > config.memory_limit {
            return UpdateCodingChallenge::memory_limit_exceeded(config.memory_limit);
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

        UpdateCodingChallenge::ok(CodingChallenge::from(cc, subtask))
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

impl Api {
    fn get_judge<'a>(&'a self, evaluator: &'a str) -> Judge<'a> {
        Judge {
            sandkasten: &self.sandkasten,
            evaluator,
            cache: &self.judge_cache,
        }
    }
}
