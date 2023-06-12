use fnct::format::JsonFormatter;
use lib::{auth::VerifiedUserAuth, Cache};
use poem::web::Data;
use poem_ext::{db::DbTxn, response};
use poem_openapi::{param::Path, payload::Json, OpenApi};
use sandkasten_client::{schemas::programs::RunResult, SandkastenClient};
use tracing::error;
use uuid::Uuid;

use super::get_challenge;
use crate::{
    endpoints::Tags,
    schemas::coding_challenges::{CheckResult, SubmissionContent},
    services::judge::{self, Judge},
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
}

response!(TestExample = {
    Ok(200) => CheckResult<RunResult>,
    /// Example does not exist.
    ExampleNotFound(404, error),
    /// Environment does not exist.
    EnvironmentNotFound(404, error),
    /// The evaluator failed to execute.
    EvaluatorFailed(400, error),
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
