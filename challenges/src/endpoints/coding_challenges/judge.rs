use fnct::{format::JsonFormatter, key};
use lib::{auth::VerifiedUserAuth, Cache};
use poem::web::Data;
use poem_ext::{db::DbTxn, response};
use poem_openapi::{param::Path, payload::Json, OpenApi};
use sandkasten_client::{
    schemas::{environments::ListEnvironmentsResponse, programs::RunResult},
    SandkastenClient,
};
use tracing::error;
use uuid::Uuid;

use super::get_challenge;
use crate::{
    endpoints::Tags,
    schemas::coding_challenges::{CheckResult, ExecutorConfig, SubmissionContent},
    services::{
        judge::{self, get_executor_config, Judge},
        subtasks::{get_user_subtask, UserSubtaskExt},
    },
};

pub struct Api {
    pub sandkasten: SandkastenClient,
    pub judge_cache: Cache<JsonFormatter>,
}

#[OpenApi(tag = "Tags::CodingChallenges")]
impl Api {
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
        data: Json<SubmissionContent>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> TestExample::Response<VerifiedUserAuth> {
        let Some((cc, subtask)) = get_challenge(&db, task_id.0, subtask_id.0).await? else {
            return TestExample::example_not_found();
        };
        if !auth.0.admin && auth.0.id != subtask.creator && !subtask.enabled {
            return TestExample::example_not_found();
        }

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;
        if !user_subtask.check_access(&auth.0, &subtask) {
            return TestExample::no_access();
        }

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
                    "evaluator for {} failed to execute while generating example input for {}: \
                     {:?}",
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
                    "evaluator for {} failed to execute while testing submission for example {}: \
                     {:?}",
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

    /// Return a map of all environments available on the code execution engine.
    ///
    /// The keys represent the environment ids and the values contain additional
    /// information about the environments.
    #[oai(path = "/executor/environments", method = "get")]
    async fn list_environments(
        &self,
        _auth: VerifiedUserAuth,
    ) -> ListEnvironments::Response<VerifiedUserAuth> {
        ListEnvironments::ok(ListEnvironmentsResponse(
            self.judge_cache
                .cached_result(key!(), &[], None, || async {
                    self.sandkasten.list_environments().await
                })
                .await??,
        ))
    }

    /// Return the config of the code execution engine.
    ///
    /// The keys represent the environment ids and the values contain additional
    /// information about the environments.
    #[oai(path = "/executor/config", method = "get")]
    async fn get_config(&self, _auth: VerifiedUserAuth) -> GetConfig::Response<VerifiedUserAuth> {
        let config = get_executor_config(&self.judge_cache, &self.sandkasten).await?;
        GetConfig::ok(ExecutorConfig {
            time_limit: (config.run_limits.time - 1) * 1000,
            memory_limit: config.run_limits.memory,
        })
    }
}

response!(TestExample = {
    Ok(200) => CheckResult<RunResult>,
    /// Example does not exist.
    ExampleNotFound(404, error),
    /// Environment does not exist.
    EnvironmentNotFound(404, error),
    /// The user has not unlocked this question.
    NoAccess(403, error),
    /// The evaluator failed to execute.
    EvaluatorFailed(400, error),
});

response!(ListEnvironments = {
    /// Map of available environments.
    Ok(200) => ListEnvironmentsResponse,
});

response!(GetConfig = {
    /// Configuration of the code execution engine.
    Ok(200) => ExecutorConfig,
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
