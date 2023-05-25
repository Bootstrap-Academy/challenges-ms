use std::sync::Arc;

use chrono::Utc;
use entity::{challenges_coding_challenges, challenges_subtasks};
use fnct::format::JsonFormatter;
use lib::{
    auth::{AdminAuth, VerifiedUserAuth},
    Cache, SharedState,
};
use poem::web::Data;
use poem_ext::{db::DbTxn, response, responses::ErrorResponse};
use poem_openapi::{
    param::Path,
    payload::{Json, PlainText},
    Object, OpenApi,
};
use sandkasten_client::{
    schemas::programs::{BuildRunResult, RunResult},
    SandkastenClient,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, ModelTrait, QueryFilter,
    QueryOrder, Set, Unchanged,
};
use tracing::error;
use uuid::Uuid;

use crate::{
    schemas::coding_challenges::{
        CheckResult, CodingChallenge, CreateCodingChallengeRequest, Example, Submission,
        UpdateCodingChallengeRequest,
    },
    services::{
        judge::{self, Judge},
        tasks::get_task,
    },
};

use super::Tags;

pub struct CodingChallenges {
    pub state: Arc<SharedState>,
    pub sandkasten: SandkastenClient,
    pub judge_cache: Cache<JsonFormatter>,
}

#[OpenApi(tag = "Tags::CodingChallenges")]
impl CodingChallenges {
    /// List all coding challenges in a task.
    #[oai(path = "/tasks/:task_id/coding_challenges", method = "get")]
    async fn list_challenges(
        &self,
        task_id: Path<Uuid>,
        db: Data<&DbTxn>,
        _auth: VerifiedUserAuth,
    ) -> ListCodingChallenges::Response<VerifiedUserAuth> {
        ListCodingChallenges::ok(
            challenges_coding_challenges::Entity::find()
                .find_also_related(challenges_subtasks::Entity)
                .filter(challenges_subtasks::Column::TaskId.eq(task_id.0))
                .order_by_asc(challenges_subtasks::Column::CreationTimestamp)
                .all(&***db)
                .await?
                .into_iter()
                .filter_map(|(cc, subtask)| Some(CodingChallenge::from(cc, subtask?)))
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
        _auth: VerifiedUserAuth,
    ) -> GetCodingChallenge::Response<VerifiedUserAuth> {
        match get_challenge(&db, task_id.0, subtask_id.0).await? {
            Some((cc, subtask)) => GetCodingChallenge::ok(CodingChallenge::from(cc, subtask)),
            None => GetCodingChallenge::subtask_not_found(),
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
        _auth: VerifiedUserAuth,
    ) -> GetExamples::Response<VerifiedUserAuth> {
        let Some((cc, _)) = get_challenge(&db, task_id.0, subtask_id.0).await? else {
            return GetExamples::subtask_not_found();
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
        _auth: AdminAuth,
    ) -> GetEvaluator::Response<AdminAuth> {
        let Some((cc, _)) = get_challenge(&db, task_id.0, subtask_id.0).await? else {
            return GetEvaluator::subtask_not_found();
        };

        GetEvaluator::ok(cc.evaluator)
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
        _auth: AdminAuth,
    ) -> GetSolution::Response<AdminAuth> {
        let Some((cc, _)) = get_challenge(&db, task_id.0, subtask_id.0).await? else {
            return GetSolution::subtask_not_found();
        };

        GetSolution::ok(Submission {
            environment: cc.solution_environment,
            code: cc.solution_code,
        })
    }

    /// Create a new coding challenge.
    #[oai(path = "/tasks/:task_id/coding_challenges", method = "post")]
    async fn create_challenge(
        &self,
        task_id: Path<Uuid>,
        data: Json<CreateCodingChallengeRequest>,
        db: Data<&DbTxn>,
        auth: AdminAuth,
    ) -> CreateCodingChallenge::Response<AdminAuth> {
        let task = match get_task(&db, task_id.0).await? {
            Some(task) => task,
            None => return CreateCodingChallenge::task_not_found(),
        };

        let cc_id = Uuid::new_v4();
        if let Err(result) = self
            .check_challenge(CheckChallenge {
                challenge_id: cc_id,
                evaluator: &data.0.evaluator,
                solution_environment: &data.0.solution_environment,
                solution_code: &data.0.solution_code,
                time_limit: data.0.time_limit,
                memory_limit: data.0.memory_limit,
                static_tests: data.0.static_tests,
                random_tests: data.0.random_tests,
            })
            .await?
        {
            return Ok(result.into());
        }

        let subtask = challenges_subtasks::ActiveModel {
            id: Set(cc_id),
            task_id: Set(task.id),
            creator: Set(auth.0.id),
            creation_timestamp: Set(Utc::now().naive_utc()),
            xp: Set(data.0.xp),
            coins: Set(data.0.coins),
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
        _auth: AdminAuth,
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

        if let Err(result) = self
            .check_challenge(CheckChallenge {
                challenge_id: cc.subtask_id,
                evaluator: data.0.evaluator.get_new(&cc.evaluator),
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
            return Ok(result.into());
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
            xp: data.0.xp.update(subtask.xp),
            coins: data.0.coins.update(subtask.coins),
        }
        .update(&***db)
        .await?;

        UpdateCodingChallenge::ok(CodingChallenge::from(cc, subtask))
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

    /// Test a solution against an example.
    #[oai(
        path = "/tasks/:task_id/coding_challenges/:subtask_id/examples/:example_id/test",
        method = "post"
    )]
    async fn test_example(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        example_id: Path<String>,
        data: Json<Submission>,
        db: Data<&DbTxn>,
        _auth: VerifiedUserAuth,
    ) -> TestExample::Response<VerifiedUserAuth> {
        let Some((cc, _)) = get_challenge(&db, task_id.0, subtask_id.0).await? else {
            return TestExample::example_not_found();
        };
        let judge = self.get_judge(&cc.evaluator);

        let examples = match judge.examples().await {
            Err(judge::Error::EvaluatorFailed(err) | judge::Error::InvalidOutput(err)) => {
                error!(
                    "evaluator for {} failed to execute while listing examples: {:?}",
                    subtask_id.0, err
                );
                return TestExample::evaluator_failed();
            }
            x => x?,
        };
        if !examples.contains(&example_id.0) {
            return TestExample::example_not_found();
        }

        let inp = match judge.generate(&example_id.0).await {
            Err(judge::Error::EvaluatorFailed(err) | judge::Error::InvalidOutput(err)) => {
                error!(
                    "evaluator for {} failed to execute while generating example input for {}: {:?}",
                    subtask_id.0, example_id.0, err
                );
                return TestExample::evaluator_failed();
            }
            x => x?,
        };

        let result = match judge
            .run_solution(
                &example_id.0,
                &inp,
                &data.0.environment,
                &data.0.code,
                Some(cc.time_limit as _),
                Some(cc.memory_limit as _),
            )
            .await
        {
            Err(judge::Error::EvaluatorFailed(err) | judge::Error::InvalidOutput(err)) => {
                error!(
                    "evaluator for {} failed to execute while testing submission for example {}: {:?}",
                    subtask_id.0, example_id.0, err
                );
                return TestExample::evaluator_failed();
            }
            Err(judge::Error::EnvironmentNotFound) => {
                return TestExample::environment_not_found();
            }
            x => x?,
        };

        TestExample::ok(result)
    }

    /// Return the evaluator template.
    #[oai(path = "/coding_challenges/evaluator/template.py", method = "get")]
    async fn get_evaluator_template(&self) -> PlainText<&'static str> {
        PlainText(include_str!("../../assets/evaluator/template.py"))
    }

    /// Return the evaluator library.
    #[oai(path = "/coding_challenges/evaluator/lib.py", method = "get")]
    async fn get_evaluator_lib(&self) -> PlainText<&'static str> {
        PlainText(include_str!("../../assets/evaluator/lib.py"))
    }
}

response!(ListCodingChallenges = {
    Ok(200) => Vec<CodingChallenge>,
});

response!(GetCodingChallenge = {
    Ok(200) => CodingChallenge,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
});

response!(GetExamples = {
    Ok(200) => Vec<Example>,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The evaluator failed to execute.
    EvaluatorFailed(400, error),
    /// Failed to generate an example.
    ExampleGenerationFailed(400, error),
});

response!(GetEvaluator = {
    Ok(200) => String,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
});

response!(GetSolution = {
    Ok(200) => Submission,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
});

response!(CreateCodingChallenge = {
    Ok(201) => CodingChallenge,
    /// Task does not exist.
    TaskNotFound(404, error),
    ..CheckError::Response,
});

response!(UpdateCodingChallenge = {
    Ok(200) => CodingChallenge,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// Task does not exist.
    TaskNotFound(404, error),
    ..CheckError::Response,
});

response!(DeleteCodingChallenge = {
    Ok(200),
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
});

response!(TestExample = {
    Ok(200) => CheckResult<RunResult>,
    /// Example does not exist.
    ExampleNotFound(404, error),
    /// Environment does not exist.
    EnvironmentNotFound(404, error),
    /// The evaluator failed to execute.
    EvaluatorFailed(400, error),
});

impl CodingChallenges {
    fn get_judge<'a>(&'a self, evaluator: &'a str) -> Judge<'a> {
        Judge {
            sandkasten: &self.sandkasten,
            evaluator,
            cache: &self.judge_cache,
        }
    }

    async fn check_challenge(
        &self,
        CheckChallenge {
            challenge_id,
            evaluator,
            solution_environment,
            solution_code,
            time_limit,
            memory_limit,
            static_tests,
            random_tests,
        }: CheckChallenge<'_>,
    ) -> Result<Result<(), CheckError::Response>, ErrorResponse> {
        let judge = self.get_judge(evaluator);

        let examples = match judge.examples().await {
            Err(judge::Error::EvaluatorFailed(err)) => {
                return Ok(Err(CheckError::evaluator_failed(err)));
            }
            Err(judge::Error::InvalidOutput(err)) => {
                return Ok(Err(CheckError::invalid_output(err)));
            }
            x => x?,
        };
        if examples.is_empty() {
            return Ok(Err(CheckError::no_examples()));
        }

        for seed in examples
            .into_iter()
            .chain((0..static_tests).map(|x| format!("_static_{x}_{challenge_id}")))
            .chain((0..random_tests).map(|_| Uuid::new_v4().to_string()))
        {
            let result = match judge
                .get_example_checked(
                    &seed,
                    solution_environment,
                    solution_code,
                    Some(time_limit),
                    Some(memory_limit),
                )
                .await
            {
                Err(judge::Error::EnvironmentNotFound) => {
                    return Ok(Err(CheckError::environment_not_found()));
                }
                Err(judge::Error::EvaluatorFailed(err)) => {
                    return Ok(Err(CheckError::evaluator_failed(err)));
                }
                Err(judge::Error::InvalidOutput(err)) => {
                    return Ok(Err(CheckError::invalid_output(err)));
                }
                x => x?,
            };
            if let Err(result) = result {
                return Ok(Err(CheckError::testcase_failed(CheckTestcaseError {
                    seed: seed.clone(),
                    result,
                })));
            }
        }

        Ok(Ok(()))
    }
}

async fn get_challenge(
    db: &DatabaseTransaction,
    task_id: Uuid,
    subtask_id: Uuid,
) -> Result<
    Option<(
        challenges_coding_challenges::Model,
        challenges_subtasks::Model,
    )>,
    ErrorResponse,
> {
    Ok(
        match challenges_coding_challenges::Entity::find_by_id(subtask_id)
            .find_also_related(challenges_subtasks::Entity)
            .filter(challenges_subtasks::Column::TaskId.eq(task_id))
            .one(db)
            .await?
        {
            Some((cc, Some(subtask))) => Some((cc, subtask)),
            _ => None,
        },
    )
}

struct CheckChallenge<'a> {
    challenge_id: Uuid,
    evaluator: &'a str,
    solution_environment: &'a str,
    solution_code: &'a str,
    time_limit: u64,
    memory_limit: u64,
    static_tests: u8,
    random_tests: u8,
}

mod _check_error {
    use super::*;
    response!(pub CheckError = {
        /// The list of examples provided by the evaluator is empty.
        NoExamples(404, error),
        /// The solution environment does not exist.
        EnvironmentNotFound(404, error),
        /// The evaluator crashed.
        EvaluatorFailed(400, error) => BuildRunResult,
        /// The evaluator failed to produce valid output.
        InvalidOutput(400, error) => BuildRunResult,
        /// The sample solution failed on a specific test case.
        TestcaseFailed(400, error) => CheckTestcaseError,
    });
}
use _check_error::CheckError::raw as CheckError;

#[derive(Debug, Object)]
pub struct CheckTestcaseError {
    seed: String,
    result: CheckResult<RunResult>,
}
