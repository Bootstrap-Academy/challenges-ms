use std::{future::Future, sync::Arc, time::Duration};

use entity::challenges_coding_challenges;
use fnct::{format::JsonFormatter, key};
use lib::{auth::VerifiedUserAuth, config::Config, Cache, SharedState};
use poem::{http::StatusCode, web::Data, Response};
use poem_ext::{db::DbTxn, response, responses::InnerResponse};
use poem_openapi::{param::Path, payload::Json, ApiResponse, Object, OpenApi};
use sandkasten_client::{
    schemas::{environments::ListEnvironmentsResponse, programs::RunResult},
    SandkastenClient,
};
use schemas::challenges::coding_challenges::{CheckResult, ExecutorConfig, SubmissionContent};
use sea_orm::{DatabaseConnection, TransactionTrait};
use tracing::error;
use uuid::Uuid;

use crate::{
    endpoints::Tags,
    services::{
        coding_execution::InlineRun,
        judge::{self, get_executor_config, Judge},
        subtasks::get_subtask,
    },
};

pub struct Api {
    pub state: Arc<SharedState>,
    pub config: Arc<Config>,
    pub inline_db: DatabaseConnection,
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
        self.with_deadline(self.test_example_inner(task_id, subtask_id, example_id, data, db, auth))
            .await
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
    #[oai(path = "/executor/config", method = "get")]
    async fn get_config(&self, _auth: VerifiedUserAuth) -> GetConfig::Response<VerifiedUserAuth> {
        GetConfig::ok(get_executor_config(&self.judge_cache, &self.sandkasten).await?)
    }

    /// Scoped retained learning only; no ordinary session or publication authority.
    #[oai(
        path = "/learning/tasks/:task_id/coding_challenges/:subtask_id/examples/:example_id/test",
        method = "post"
    )]
    #[allow(clippy::too_many_arguments)]
    async fn learning_test_example(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        example_id: Path<String>,
        data: Json<SubmissionContent>,
        db: Data<&DbTxn>,
        auth: lib::auth::LearningAuth,
    ) -> TestExample::Response<VerifiedUserAuth> {
        self.with_deadline(async {
            // Scoped admission locks the subject. Release its short transaction
            // before the separately committed reservation takes the same lock.
            let admission = self.inline_db.begin().await?;
            let user =
                crate::services::learning::admit(&admission, &self.state.services, auth.0).await?;
            admission.commit().await?;
            // Scoped admission and every executor phase share the same deadline.
            self.test_example_inner(
                task_id,
                subtask_id,
                example_id,
                data,
                db,
                VerifiedUserAuth(user),
            )
            .await
        })
        .await
    }

    /// Scoped retained learning only; no ordinary session or publication authority.
    #[oai(path = "/learning/executor/environments", method = "get")]
    #[allow(clippy::too_many_arguments)]
    async fn learning_list_environments(
        &self,
        _auth: lib::auth::LearningAuth,
        db: Data<&DbTxn>,
    ) -> ListEnvironments::Response<VerifiedUserAuth> {
        let user = crate::services::learning::admit(&db, &self.state.services, _auth.0).await?;
        // Reuse product behavior after dedicated scoped admission. This local
        // wrapper value does not pass through any ordinary HTTP authenticator.
        self.list_environments(VerifiedUserAuth(user)).await
    }

    /// Scoped retained learning only; no ordinary session or publication authority.
    #[oai(path = "/learning/executor/config", method = "get")]
    #[allow(clippy::too_many_arguments)]
    async fn learning_get_config(
        &self,
        _auth: lib::auth::LearningAuth,
        db: Data<&DbTxn>,
    ) -> GetConfig::Response<VerifiedUserAuth> {
        let user = crate::services::learning::admit(&db, &self.state.services, _auth.0).await?;
        // Reuse product behavior after dedicated scoped admission. This local
        // wrapper value does not pass through any ordinary HTTP authenticator.
        self.get_config(VerifiedUserAuth(user)).await
    }
}

response!(TestExample = {
    Ok(200) => CheckResult<RunResult>,
    /// Example does not exist.
    ExampleNotFound(404, error),
    /// Environment does not exist.
    EnvironmentNotFound(404, error),
    /// The user does not have enough hearts to submit a solution and is neither an admin nor the creator of this subtask.
    NotEnoughHearts(403, error),
    /// Shared global or personal execution admission is full; retry costs nothing.
    TooManyRequests(429) => InlineRetry,
    /// A technical failure stopped the test; retry costs nothing.
    ExecutionUnavailable(503) => InlineRetry,
});

#[derive(Debug, Object)]
struct InlineRetry {
    error: String,
    detail: String,
    retry_after: u64,
}

response!(ListEnvironments = {
    /// Map of available environments.
    Ok(200) => ListEnvironmentsResponse,
});

response!(GetConfig = {
    /// Configuration of the code execution engine.
    Ok(200) => ExecutorConfig,
});

impl Api {
    async fn with_deadline(
        &self,
        work: impl Future<Output = TestExample::Response<VerifiedUserAuth>>,
    ) -> TestExample::Response<VerifiedUserAuth> {
        let settings = &self.config.challenges.coding_challenges.execution;
        match tokio::time::timeout(
            Duration::from_secs(u64::from(settings.max_execution_seconds)),
            work,
        )
        .await
        {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(err)) => {
                error!("inline coding test failed: {err:?}");
                self.execution_unavailable()
            }
            Err(_) => {
                error!("inline coding test exceeded its execution deadline");
                self.execution_unavailable()
            }
        }
    }

    fn execution_unavailable(&self) -> TestExample::Response<VerifiedUserAuth> {
        self.retry_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "coding_execution_unavailable",
            "Der Test konnte gerade nicht fertig werden. Versuch es gleich noch einmal – das kostet dich keine Herzen oder Versuche.",
        )
    }

    fn retry_response(
        &self,
        status: StatusCode,
        code: &str,
        detail: &str,
    ) -> TestExample::Response<VerifiedUserAuth> {
        let retry_after = self
            .config
            .challenges
            .coding_challenges
            .execution
            .retry_seconds;
        let response = Response::builder()
            .status(status)
            .header("Retry-After", retry_after.to_string())
            .content_type("application/json")
            .body(
                serde_json::json!({"error":code,"detail":detail,"retry_after":retry_after})
                    .to_string(),
            );
        Ok(InnerResponse::from_parse_request_error(
            poem::Error::from_response(response),
        ))
    }

    async fn test_example_inner(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        example_id: Path<String>,
        data: Json<SubmissionContent>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> TestExample::Response<VerifiedUserAuth> {
        let Some((cc, subtask)) =
            get_subtask::<challenges_coding_challenges::Entity>(&db, task_id.0, subtask_id.0)
                .await?
        else {
            return TestExample::example_not_found();
        };
        if !auth.0.admin
            && (subtask.moderation_removed || (auth.0.id != subtask.creator && !subtask.enabled))
        {
            return TestExample::example_not_found();
        }
        match crate::services::hearts::admit(&db, &self.state.services, &auth.0, &subtask).await? {
            crate::services::hearts::Admission::Allowed { .. } => {}
            crate::services::hearts::Admission::NoHearts => {
                return TestExample::not_enough_hearts();
            }
            crate::services::hearts::Admission::Unavailable(denial) => return denial.response(),
        }
        if !crate::services::access::can_read_subtask(&db, &self.state.services, &auth.0, &subtask)
            .await?
        {
            return TestExample::example_not_found();
        }

        let settings = &self.config.challenges.coding_challenges.execution;
        let Some(mut reservation) =
            InlineRun::reserve(&self.inline_db, auth.0.id, subtask.id, settings).await?
        else {
            return self.retry_response(
                StatusCode::TOO_MANY_REQUESTS,
                "coding_execution_busy",
                "Es laufen gerade zu viele Tests. Versuch es gleich noch einmal – das kostet dich keine Herzen oder Versuche.",
            );
        };

        let work = async {
            let judge = self.get_judge(&cc.evaluator);
            if !judge.examples().await?.contains(&example_id.0) {
                return TestExample::example_not_found();
            }
            let input = judge.generate(&example_id.0).await?;
            // Keep the concrete daily admission before learner execution.
            if let Some(denial) = crate::services::access::start(
                &db,
                &self.state.services,
                &auth.0,
                &subtask,
                Uuid::new_v4(),
            )
            .await?
            {
                return denial.response();
            }
            match judge
                .run_solution(
                    &example_id.0,
                    &input,
                    &data.0.environment,
                    &data.0.code,
                    Some(cc.time_limit as _),
                    Some(cc.memory_limit as _),
                )
                .await
            {
                Err(judge::Error::EnvironmentNotFound) => TestExample::environment_not_found(),
                result => TestExample::ok(result?),
            }
        };
        let result = tokio::select! {
            result = work => result,
            removed = reservation.until_removed(settings.poll_milliseconds) => {
                removed?;
                self.execution_unavailable()
            },
        };
        // Also inside the outer deadline. Drop retries cleanup if this await is
        // cancelled; the expiry fences process loss or an unavailable database.
        reservation.release().await?;
        result
    }

    fn get_judge<'a>(&'a self, evaluator: &'a str) -> Judge<'a> {
        Judge {
            sandkasten: &self.sandkasten,
            evaluator,
            cache: &self.judge_cache,
        }
    }
}

#[cfg(test)]
#[path = "inline_tests.rs"]
mod tests;
