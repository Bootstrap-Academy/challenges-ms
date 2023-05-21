use fnct::{format::JsonFormatter, key};
use lib::{Cache, CacheError};
use sandkasten_client::{
    schemas::programs::{BuildRequest, BuildRunError, BuildRunRequest, MainFile, RunRequest},
    SandkastenClient,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::schemas::coding_challenges::EvaluatorError;

pub struct Judge<'a> {
    pub sandkasten: &'a SandkastenClient,
    pub evaluator: &'a str,
    pub cache: &'a Cache<JsonFormatter>,
}

impl Judge<'_> {
    async fn exec<I: Serialize, O: DeserializeOwned>(
        &self,
        args: Vec<String>,
        stdin: Option<I>,
    ) -> Result<O, Error> {
        let out = self
            .sandkasten
            .build_and_run(&BuildRunRequest {
                build: BuildRequest {
                    environment: "python".into(),
                    main_file: MainFile {
                        content: self.evaluator.to_owned(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                run: RunRequest {
                    args,
                    stdin: stdin.map(|s| serde_json::to_string(&s)).transpose()?,
                    ..Default::default()
                },
            })
            .await?;
        if out.run.status != 0 {
            return Err(Error::ExecutionFailed(EvaluatorError {
                code: out.run.status,
                stderr: out.run.stderr,
            }));
        }
        Ok(serde_json::from_str(&out.run.stdout)?)
    }

    pub async fn examples(&self) -> Result<Vec<String>, Error> {
        self.cache
            .cached_result(key!(self.evaluator), &[], None, async {
                self.exec(vec!["examples".into()], None::<()>).await
            })
            .await?
    }

    pub async fn generate(&self, seed: &str) -> Result<Input, Error> {
        self.cache
            .cached_result(key!(self.evaluator, seed), &[], None, async {
                self.exec(vec!["generate".into(), seed.into()], None::<()>)
                    .await
            })
            .await?
    }

    pub async fn check(&self, seed: &str, output: &Output<'_>) -> Result<Verdict, Error> {
        self.exec(vec!["check".into(), seed.into()], Some(output))
            .await
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("cache error: {0}")]
    Cache(#[from] CacheError<JsonFormatter>),
    #[error("sandkasten error: {0}")]
    Sandkasten(#[from] sandkasten_client::Error<BuildRunError>),
    #[error("serde_json error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("failed to execute evaluator: {0:?}")]
    ExecutionFailed(EvaluatorError),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Input {
    pub input: String,
    pub data: Value,
}

#[derive(Debug, Serialize)]
pub struct Output<'a> {
    pub output: &'a str,
    pub data: &'a Value,
}

#[derive(Debug, Deserialize)]
pub struct Verdict {
    pub ok: bool,
    pub reason: String,
}
