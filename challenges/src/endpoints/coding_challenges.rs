use std::sync::Arc;

use chrono::Utc;
use entity::{
    challenges_coding_challenge_example, challenges_coding_challenges, challenges_subtasks,
};
use lib::{
    auth::{AdminAuth, VerifiedUserAuth},
    SharedState,
};
use poem::web::Data;
use poem_ext::{db::DbTxn, response, responses::ErrorResponse};
use poem_openapi::{param::Path, payload::Json, OpenApi};
use sandkasten_client::{
    schemas::programs::{BuildRequest, BuildRunError, BuildRunRequest, MainFile, RunRequest},
    Error as SandkastenError, SandkastenClient,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, ModelTrait, QueryFilter,
    QueryOrder, Set, Unchanged,
};
use uuid::Uuid;

use crate::{
    schemas::coding_challenges::{
        CodingChallenge, CreateCodingChallengeRequest, CreateExampleRequest, EvaluatorError,
        Example, Submission, SubmissionResult, UpdateCodingChallengeRequest, UpdateExampleRequest,
        Verdict,
    },
    services::{
        judge::{self, Judge, Output},
        tasks::get_task,
    },
};

use super::Tags;

pub struct CodingChallenges {
    pub state: Arc<SharedState>,
    pub sandkasten: SandkastenClient,
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

        GetExamples::ok(
            cc.find_related(challenges_coding_challenge_example::Entity)
                .all(&***db)
                .await?
                .into_iter()
                .map(Into::into)
                .collect(),
        )
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
        let subtask = challenges_subtasks::ActiveModel {
            id: Set(Uuid::new_v4()),
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
            evaluator: Set(data.0.evaluator),
            description: Set(data.0.description),
        }
        .insert(&***db)
        .await?;
        challenges_coding_challenge_example::Entity::insert_many(data.0.examples.into_iter().map(
            |e| challenges_coding_challenge_example::ActiveModel {
                id: Set(Uuid::new_v4()),
                challenge_id: Set(cc.subtask_id),
                input: Set(e.input),
                output: Set(e.output),
                explanation: Set(e.explanation),
            },
        ))
        .exec(&***db)
        .await?;
        CreateCodingChallenge::ok(CodingChallenge::from(cc, subtask))
    }

    /// Create a new example for a coding challenge.
    #[oai(
        path = "/tasks/:task_id/coding_challenges/:subtask_id/examples",
        method = "post"
    )]
    async fn create_example(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        data: Json<CreateExampleRequest>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> CreateExample::Response<AdminAuth> {
        let Some((cc, _)) = get_challenge(&db, task_id.0, subtask_id.0).await? else {
            return CreateExample::subtask_not_found();
        };

        CreateExample::ok(
            challenges_coding_challenge_example::ActiveModel {
                id: Set(Uuid::new_v4()),
                challenge_id: Set(cc.subtask_id),
                input: Set(data.0.input),
                output: Set(data.0.output),
                explanation: Set(data.0.explanation),
            }
            .insert(&***db)
            .await?
            .into(),
        )
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

        let cc = challenges_coding_challenges::ActiveModel {
            subtask_id: Unchanged(cc.subtask_id),
            time_limit: data.0.time_limit.map(|x| x as _).update(cc.time_limit),
            memory_limit: data.0.memory_limit.map(|x| x as _).update(cc.memory_limit),
            evaluator: data.0.evaluator.update(cc.evaluator),
            description: data.0.description.update(cc.description),
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

    /// Update an example of a coding challenge.
    #[oai(
        path = "/tasks/:task_id/coding_challenges/:subtask_id/examples/:example_id",
        method = "patch"
    )]
    async fn update_example(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        example_id: Path<Uuid>,
        data: Json<UpdateExampleRequest>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> UpdateExample::Response<AdminAuth> {
        let Some((example, _, _)) = get_example(&db, task_id.0, subtask_id.0, example_id.0).await? else {
            return UpdateExample::example_not_found();
        };

        UpdateExample::ok(
            challenges_coding_challenge_example::ActiveModel {
                id: Unchanged(example.id),
                challenge_id: Unchanged(example.challenge_id),
                input: data.0.input.update(example.input),
                output: data.0.output.update(example.output),
                explanation: data.0.explanation.update(example.explanation),
            }
            .update(&***db)
            .await?
            .into(),
        )
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

    /// Delete an example of a coding challenge.
    #[oai(
        path = "/tasks/:task_id/coding_challenges/:subtask_id/examples/:example_id",
        method = "delete"
    )]
    async fn delete_example(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        example_id: Path<Uuid>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> DeleteExample::Response<AdminAuth> {
        match get_example(&db, task_id.0, subtask_id.0, example_id.0).await? {
            Some((example, _, _)) => {
                example.delete(&***db).await?;
                DeleteExample::ok()
            }
            None => DeleteExample::example_not_found(),
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
        example_id: Path<usize>,
        data: Json<Submission>,
        db: Data<&DbTxn>,
        _auth: VerifiedUserAuth,
    ) -> TestExample::Response<VerifiedUserAuth> {
        let Some((cc, _)) = get_challenge(&db, task_id.0, subtask_id.0).await? else {
            return TestExample::example_not_found();
        };

        let judge = Judge {
            sandkasten: &self.sandkasten,
            evaluator: &cc.evaluator,
        };

        let examples = match judge.examples().await {
            Err(judge::Error::ExecutionFailed(err)) => return TestExample::evaluator_failed(err),
            x => x?,
        };
        let seed = &examples[example_id.0];
        let inp = match judge.generate(seed).await {
            Err(judge::Error::ExecutionFailed(err)) => return TestExample::evaluator_failed(err),
            x => x?,
        };

        let out = match self
            .sandkasten
            .build_and_run(&BuildRunRequest {
                build: BuildRequest {
                    environment: data.0.environment,
                    main_file: MainFile {
                        content: data.0.solution,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                run: RunRequest {
                    stdin: Some(inp.input),
                    ..Default::default()
                },
            })
            .await
        {
            Err(SandkastenError::ErrorResponse(err)) => {
                let sandkasten_client::schemas::ErrorResponse::Inner(err) = *err else {
                    return Err(SandkastenError::ErrorResponse(err).into());
                };
                match err {
                    BuildRunError::EnvironmentNotFound => {
                        return TestExample::environment_not_found()
                    }
                    BuildRunError::CompileError(result) => {
                        return TestExample::ok(SubmissionResult {
                            verdict: Verdict::CompilationError,
                            reason: None,
                            build_stderr: Some(result.stderr),
                            build_time: Some(result.resource_usage.time),
                            build_memory: Some(result.resource_usage.memory),
                            run_stderr: None,
                            run_time: None,
                            run_memory: None,
                        })
                    }
                    _ => {
                        return Err(SandkastenError::ErrorResponse(
                            sandkasten_client::schemas::ErrorResponse::Inner(err).into(),
                        )
                        .into())
                    }
                }
            }
            x => x?,
        };

        let (verdict, reason) = if out.run.status != 0 {
            (Verdict::RuntimeError, None)
        } else if out.run.stdout.is_empty() {
            (Verdict::NoOutput, None)
        } else {
            let result = match judge
                .check(
                    seed,
                    &Output {
                        output: &out.run.stdout,
                        data: &inp.data,
                    },
                )
                .await
            {
                Err(judge::Error::ExecutionFailed(err)) => {
                    return TestExample::evaluator_failed(err)
                }
                x => x?,
            };
            let verdict = if result.ok {
                Verdict::Ok
            } else {
                Verdict::WrongAnswer
            };
            (verdict, Some(result.reason))
        };

        let (build_stderr, build_time, build_memory) =
            out.build.map_or_else(Default::default, |x| {
                (
                    Some(x.stderr),
                    Some(x.resource_usage.time),
                    Some(x.resource_usage.memory),
                )
            });

        TestExample::ok(SubmissionResult {
            verdict,
            reason,
            build_stderr,
            build_time,
            build_memory,
            run_stderr: Some(out.run.stderr),
            run_time: Some(out.run.resource_usage.time),
            run_memory: Some(out.run.resource_usage.memory),
        })
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
});

response!(GetEvaluator = {
    Ok(200) => String,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
});

response!(CreateCodingChallenge = {
    Ok(201) => CodingChallenge,
    /// Task does not exist.
    TaskNotFound(404, error),
});

response!(CreateExample = {
    Ok(201) => Example,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
});

response!(UpdateCodingChallenge = {
    Ok(200) => CodingChallenge,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// Task does not exist.
    TaskNotFound(404, error),
});

response!(UpdateExample = {
    Ok(200) => Example,
    /// Example does not exist.
    ExampleNotFound(404, error),
});

response!(DeleteCodingChallenge = {
    Ok(200),
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
});

response!(DeleteExample = {
    Ok(200),
    /// Example does not exist.
    ExampleNotFound(404, error),
});

response!(TestExample = {
    Ok(200) => SubmissionResult,
    /// Example does not exist.
    ExampleNotFound(404, error),
    /// Environment does not exist.
    EnvironmentNotFound(404, error),
    /// The evaluator failed to execute.
    EvaluatorFailed(400, error) => EvaluatorError,
});

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

async fn get_example(
    db: &DatabaseTransaction,
    task_id: Uuid,
    subtask_id: Uuid,
    example_id: Uuid,
) -> Result<
    Option<(
        challenges_coding_challenge_example::Model,
        challenges_coding_challenges::Model,
        challenges_subtasks::Model,
    )>,
    ErrorResponse,
> {
    let Some((cc, subtask)) = get_challenge(db, task_id, subtask_id).await? else {
        return Ok(None);
    };

    let Some(example) = challenges_coding_challenge_example::Entity::find_by_id(example_id)
        .filter(challenges_coding_challenge_example::Column::ChallengeId.eq(cc.subtask_id))
        .one(db)
        .await?
    else {
        return Ok(None);
    };

    Ok(Some((example, cc, subtask)))
}
