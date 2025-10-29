use anyhow::{bail, Context};
use entity::sea_orm_active_enums::ChallengesVerdict;
use fnct::{format::JsonFormatter, key};
use lib::{Cache, CacheError};
use sandkasten_client::{
    schemas::{
        programs::{
            BuildRequest, BuildRunError, BuildRunRequest, BuildRunResult, EnvVar, File, LimitsOpt,
            MainFile, RunRequest, RunResult,
        },
        ErrorResponse,
    },
    Error as SandkastenError, SandkastenClient,
};
use schemas::challenges::coding_challenges::{CheckResult, Example, ExecutorConfig};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use super::verdict_message::{build_message, VerdictMessageContext};

pub const EVALUATOR_TEMPLATE: &str = include_str!("../../assets/evaluator/template.py");
pub const EVALUATOR_LIBRARY: &str = include_str!("../../assets/evaluator/lib.py");
const JAVA_SMOKE_TEST: &str = r#"
class Main {
    public static void main(String[] args) {
        System.out.println("ok");
    }
}
"#;

pub struct Judge<'a> {
    pub sandkasten: &'a SandkastenClient,
    pub evaluator: &'a str,
    pub cache: &'a Cache<JsonFormatter>,
}

impl Judge<'_> {
    pub async fn get_example_checked(
        &self,
        seed: &str,
        solution_environment: &str,
        solution_code: &str,
        time_limit: Option<u64>,
        memory_limit: Option<u64>,
    ) -> Result<Result<Example, CheckResult<RunResult>>, Error> {
        self.cache
            .cached_result(
                key!(
                    self.evaluator,
                    seed,
                    solution_environment,
                    solution_code,
                    time_limit,
                    memory_limit
                ),
                &[],
                None,
                || async {
                    let input = self.generate(seed).await?;
                    let result = self
                        .run_solution(
                            seed,
                            &input,
                            solution_environment,
                            solution_code,
                            time_limit,
                            memory_limit,
                        )
                        .await?;
                    Ok(match result {
                        CheckResult {
                            verdict: ChallengesVerdict::Ok,
                            run: Some(run),
                            ..
                        } => Ok(Example {
                            id: seed.into(),
                            input: input.input,
                            output: run.stdout,
                            explanation: (!run.stderr.is_empty()).then_some(run.stderr),
                        }),
                        _ => Err(result),
                    })
                },
            )
            .await?
    }

    pub async fn examples(&self) -> Result<Vec<String>, Error> {
        self.cache
            .cached_result(key!(self.evaluator), &[], None, || async {
                self.run_evaluator(vec!["examples".into()], None::<()>)
                    .await
            })
            .await?
    }

    pub async fn generate(&self, seed: &str) -> Result<Input, Error> {
        self.cache
            .cached_result(key!(self.evaluator, seed), &[], None, || async {
                self.run_evaluator(vec!["generate".into(), seed.into()], None::<()>)
                    .await
            })
            .await?
    }

    async fn prepare(&self, seed: &str, data: &PrepareRequest<'_>) -> Result<PrepareResult, Error> {
        self.run_evaluator(vec!["prepare".into(), seed.into()], Some(data))
            .await
    }

    async fn check(&self, seed: &str, output: &Output<'_>) -> Result<EvaluatorCheckOutput, Error> {
        self.run_evaluator(vec!["check".into(), seed.into()], Some(output))
            .await
    }

    async fn run_evaluator<I: Serialize, O: DeserializeOwned>(
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
                    files: vec![File {
                        name: "lib.py".into(),
                        content: EVALUATOR_LIBRARY.into(),
                    }],
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
            return Err(Error::EvaluatorFailed(out));
        }
        serde_json::from_str(&out.run.stdout).map_err(|_| Error::InvalidOutput(out))
    }

    pub async fn run_solution(
        &self,
        seed: &str,
        input: &Input,
        environment: &str,
        code: &str,
        time_limit: Option<u64>,   // ms
        memory_limit: Option<u64>, // mb
    ) -> Result<CheckResult<RunResult>, Error> {
        let prepare_result = self
            .prepare(
                seed,
                &PrepareRequest {
                    environment,
                    code,
                    data: &input.data,
                },
            )
            .await?;
        let code = match prepare_result.code {
            Some(code) => code,
            None => {
                let reason = prepare_result.reason;
                return Ok(CheckResult {
                    verdict: ChallengesVerdict::PreCheckFailed,
                    reason: Some(reason.clone()),
                    compile: None,
                    run: None,
                    message: build_message(VerdictMessageContext {
                        verdict: ChallengesVerdict::PreCheckFailed,
                        reason: Some(reason.as_str()),
                        compile_status: None,
                        compile_stderr: None,
                        run_status: None,
                        run_stderr: None,
                        run_time_ms: None,
                        run_memory_kib: None,
                        time_limit_ms: time_limit,
                        memory_limit_mb: memory_limit,
                    }),
                });
            }
        };

        let runtime_env = java_env_vars(environment, memory_limit);
        let output = match self
            .sandkasten
            .build_and_run(&BuildRunRequest {
                build: BuildRequest {
                    environment: environment.into(),
                    main_file: MainFile {
                        content: code,
                        ..Default::default()
                    },
                    env_vars: runtime_env.clone(),
                    ..Default::default()
                },
                run: RunRequest {
                    stdin: Some(input.input.clone()),
                    env_vars: runtime_env,
                    run_limits: LimitsOpt {
                        time: time_limit.map(|x| x / 1000 + 1),
                        memory: memory_limit,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            })
            .await
        {
            Err(SandkastenError::ErrorResponse(err)) => {
                return match *err {
                    ErrorResponse::Inner(BuildRunError::EnvironmentNotFound) => {
                        Err(Error::EnvironmentNotFound)
                    }
                    ErrorResponse::Inner(BuildRunError::CompileError(result)) => {
                        let message = build_message(VerdictMessageContext {
                            verdict: ChallengesVerdict::CompilationError,
                            reason: None,
                            compile_status: Some(result.status),
                            compile_stderr: Some(&result.stderr),
                            run_status: None,
                            run_stderr: None,
                            run_time_ms: None,
                            run_memory_kib: None,
                            time_limit_ms: time_limit,
                            memory_limit_mb: memory_limit,
                        });
                        Ok(CheckResult {
                            verdict: ChallengesVerdict::CompilationError,
                            reason: None,
                            compile: Some(result),
                            run: None,
                            message,
                        })
                    }
                    err => Err(Error::Sandkasten(SandkastenError::ErrorResponse(Box::new(
                        err,
                    )))),
                }
            }
            x => x?,
        };
        if let Some(verdict) = match (time_limit, memory_limit) {
            (Some(time_limit), _) if output.run.resource_usage.time > time_limit => {
                Some(ChallengesVerdict::TimeLimitExceeded)
            }
            (_, Some(memory_limit)) if output.run.resource_usage.memory / 1024 > memory_limit => {
                Some(ChallengesVerdict::MemoryLimitExceeded)
            }
            _ if output.run.status != 0 => Some(ChallengesVerdict::RuntimeError),
            _ if output.run.stdout.is_empty() => Some(ChallengesVerdict::NoOutput),
            _ => None,
        } {
            let message = build_message(VerdictMessageContext {
                verdict,
                reason: None,
                compile_status: output.build.as_ref().map(|r| r.status),
                compile_stderr: output.build.as_ref().map(|r| r.stderr.as_str()),
                run_status: Some(output.run.status),
                run_stderr: Some(output.run.stderr.as_str()),
                run_time_ms: Some(output.run.resource_usage.time),
                run_memory_kib: Some(output.run.resource_usage.memory),
                time_limit_ms: time_limit,
                memory_limit_mb: memory_limit,
            });
            return Ok(CheckResult {
                verdict,
                reason: None,
                compile: output.build,
                run: Some(output.run),
                message,
            });
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
        let verdict = result.verdict;
        let reason = result.reason;
        let message = build_message(VerdictMessageContext {
            verdict,
            reason: reason.as_deref(),
            compile_status: output.build.as_ref().map(|r| r.status),
            compile_stderr: output.build.as_ref().map(|r| r.stderr.as_str()),
            run_status: Some(output.run.status),
            run_stderr: Some(output.run.stderr.as_str()),
            run_time_ms: Some(output.run.resource_usage.time),
            run_memory_kib: Some(output.run.resource_usage.memory),
            time_limit_ms: time_limit,
            memory_limit_mb: memory_limit,
        });
        Ok(CheckResult {
            verdict,
            reason,
            compile: output.build,
            run: Some(output.run),
            message,
        })
    }
}

fn java_env_vars(environment: &str, memory_limit_mb: Option<u64>) -> Vec<EnvVar> {
    let env = environment.to_ascii_lowercase();
    if env.starts_with("java") {
        let mut env_vars = Vec::new();

        let heap_settings = memory_limit_mb.and_then(|limit| {
            if limit < 64 {
                None
            } else {
                let headroom = limit.saturating_sub(16);
                let suggested = ((limit as f64) * 0.6).round() as u64;
                let xmx = suggested.min(headroom).max(32);
                let xms = (xmx / 2).max(16);
                Some((xms, xmx))
            }
        });

        let tool_options = match heap_settings {
            Some((xms, xmx)) => format!(
                "-Xms{}m -Xmx{}m -Xss256k -XX:ThreadStackSize=256 -XX:+UseSerialGC",
                xms, xmx
            ),
            None => "-Xss256k -XX:ThreadStackSize=256 -XX:+UseSerialGC".into(),
        };

        env_vars.push(EnvVar {
            name: "JAVA_TOOL_OPTIONS".into(),
            value: tool_options,
        });

        env_vars.push(EnvVar {
            name: "MALLOC_ARENA_MAX".into(),
            value: "2".into(),
        });

        env_vars
    } else {
        Vec::new()
    }
}

pub async fn get_executor_config(
    cache: &Cache<JsonFormatter>,
    sandkasten: &SandkastenClient,
) -> anyhow::Result<ExecutorConfig> {
    Ok(cache
        .cached_result(key!(), &[], None, || async {
            sandkasten.get_config().await
        })
        .await??
        .into())
}

pub async fn smoke_test_java(sandkasten: &SandkastenClient) -> anyhow::Result<()> {
    let env_vars = java_env_vars("java", Some(256));
    let result = sandkasten
        .build_and_run(&BuildRunRequest {
            build: BuildRequest {
                environment: "java".into(),
                main_file: MainFile {
                    name: Some("Main.java".into()),
                    content: JAVA_SMOKE_TEST.trim().into(),
                    ..Default::default()
                },
                env_vars: env_vars.clone(),
                ..Default::default()
            },
            run: RunRequest {
                env_vars,
                ..Default::default()
            },
        })
        .await
        .context("java smoke test execution failed")?;

    if result.run.status != 0 {
        bail!(
            "java smoke test exited with status {} and stderr: {}",
            result.run.status,
            result.run.stderr
        );
    }

    Ok(())
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("cache error: {0}")]
    Cache(#[from] CacheError<JsonFormatter>),
    #[error("sandkasten error: {0}")]
    Sandkasten(#[from] sandkasten_client::Error<BuildRunError>),
    #[error("serde_json error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("environment does not exist")]
    EnvironmentNotFound,
    #[error("failed to execute evaluator: {0:?}")]
    EvaluatorFailed(BuildRunResult),
    #[error("evaluator failed to produce valid output: {0:?}")]
    InvalidOutput(BuildRunResult),
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

#[derive(Debug, Serialize)]
struct PrepareRequest<'a> {
    environment: &'a str,
    code: &'a str,
    data: &'a Value,
}

#[derive(Debug, Deserialize)]
struct PrepareResult {
    code: Option<String>,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct EvaluatorCheckOutput {
    verdict: ChallengesVerdict,
    reason: Option<String>,
}
