use fnct::{format::JsonFormatter, key};
use lib::{Cache, CacheError};
use sandkasten_client::{
    schemas::programs::{
        BuildRequest, BuildRunError, BuildRunRequest, BuildRunResult, MainFile, RunRequest,
    },
    SandkastenClient,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::schemas::coding_challenges::{EvaluatorError, Example};

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

    pub async fn get_example_checked(
        &self,
        seed: &str,
        solution_environment: &str,
        solution_code: &str,
    ) -> Result<Example, Error> {
        self.cache
            .cached_result(
                key!(self.evaluator, seed, solution_environment, solution_code),
                &[],
                None,
                async {
                    let input = self.generate(seed).await?;
                    let output = self
                        .sandkasten
                        .build_and_run(&BuildRunRequest {
                            build: BuildRequest {
                                environment: solution_environment.into(),
                                main_file: MainFile {
                                    content: solution_code.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                            run: RunRequest {
                                stdin: Some(input.input.clone()),
                                ..Default::default()
                            },
                        })
                        .await?;
                    if output.run.status != 0 || output.run.stdout.is_empty() {
                        return Err(Error::SolutionFailed(Box::new(output)));
                    }
                    let result = self
                        .check(
                            seed,
                            &Output {
                                output: &output.run.stdout,
                                data: &input.data,
                            },
                        )
                        .await?;
                    if result.ok {
                        Ok(Example {
                            input: input.input,
                            output: output.run.stdout,
                            explanation: (!output.run.stderr.is_empty())
                                .then_some(output.run.stderr),
                        })
                    } else {
                        Err(Error::WrongAnswer(result))
                    }
                },
            )
            .await?
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
    #[error("failed to run the solution code: {0:?}")]
    SolutionFailed(Box<BuildRunResult>),
    #[error("solution produced a wrong answer: {0:?}")]
    WrongAnswer(Verdict),
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
