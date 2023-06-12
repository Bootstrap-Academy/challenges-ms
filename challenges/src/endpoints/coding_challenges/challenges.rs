use chrono::Utc;
use entity::{challenges_coding_challenges, challenges_subtasks};
use fnct::format::JsonFormatter;
use lib::{
    auth::{AdminAuth, VerifiedUserAuth},
    Cache,
};
use poem::web::Data;
use poem_ext::{db::DbTxn, response};
use poem_openapi::{param::Path, payload::Json, OpenApi};
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
        CodingChallenge, CreateCodingChallengeRequest, Example, SubmissionContent,
        UpdateCodingChallengeRequest,
    },
    services::{
        judge::{self, Judge},
        tasks::get_task,
    },
};

pub struct Api {
    pub sandkasten: SandkastenClient,
    pub judge_cache: Cache<JsonFormatter>,
}

#[OpenApi(tag = "Tags::CodingChallenges")]
impl Api {
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

        GetSolution::ok(SubmissionContent {
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
    Ok(200) => SubmissionContent,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
});

response!(CreateCodingChallenge = {
    Ok(201) => CodingChallenge,
    /// Task does not exist.
    TaskNotFound(404, error),
    .._CheckError::Response,
});

response!(UpdateCodingChallenge = {
    Ok(200) => CodingChallenge,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// Task does not exist.
    TaskNotFound(404, error),
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
