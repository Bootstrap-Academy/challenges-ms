use chrono::{DateTime, Utc};
use entity::{
    challenges_coding_challenge_result, challenges_coding_challenge_submissions,
    challenges_coding_challenges, sea_orm_active_enums::ChallengesVerdict,
};
use poem_ext::patch_value::PatchValue;
use poem_openapi::{
    types::{ParseFromJSON, ToJSON, Type},
    Object,
};
use sandkasten_client::schemas::{
    configuration::PublicConfig,
    programs::{ResourceUsage, RunResult},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::subtasks::{CreateSubtaskRequest, Subtask, UpdateSubtaskRequest};

#[derive(Debug, Clone, Object)]
pub struct QueueStatus {
    /// The number of workers used to process submissions.
    pub workers: usize,
    /// The number of submissions that are currently being processed.
    pub active: usize,
    /// The number of submissions that are waiting to be picked up by a worker.
    pub waiting: usize,
}

#[derive(Debug, Clone, Object)]
pub struct CodingChallengeSummary {
    #[oai(flatten)]
    pub subtask: Subtask,
    /// The challenge description. Only available if the user has unlocked the
    /// subtask.
    pub description: Option<String>,
    /// The number of milliseconds the solution may run.
    pub time_limit: u16,
    /// The number of megabytes of memory the solution may use.
    pub memory_limit: u16,
    /// The number of static tests to run for submission evaluation.
    pub static_tests: u8,
    /// The number of random tests to run for submission evaluation.
    pub random_tests: u8,
}

#[derive(Debug, Clone, Object)]
pub struct CodingChallenge {
    #[oai(flatten)]
    pub subtask: Subtask,
    /// The challenge description.
    pub description: String,
    /// The number of milliseconds the solution may run.
    pub time_limit: u16,
    /// The number of megabytes of memory the solution may use.
    pub memory_limit: u16,
    /// The number of static tests to run for submission evaluation.
    pub static_tests: u8,
    /// The number of random tests to run for submission evaluation.
    pub random_tests: u8,
}

#[derive(Debug, Clone, Object, Serialize, Deserialize)]
pub struct Example {
    /// The unique identifier of the example.
    pub id: String,
    /// The input the program receives via stdin.
    pub input: String,
    /// The output the program should produce on stdout.
    pub output: String,
    /// An optional explanation for the example.
    pub explanation: Option<String>,
}

#[derive(Debug, Clone, Object)]
pub struct CreateCodingChallengeRequest {
    #[oai(flatten)]
    pub subtask: CreateSubtaskRequest,
    /// The challenge description.
    #[oai(validator(max_length = 16384))]
    pub description: String,
    /// The number of milliseconds the solution may run.
    #[oai(validator(minimum(value = "1")))]
    pub time_limit: u64,
    /// The number of megabytes of memory the solution may use.
    #[oai(validator(minimum(value = "1")))]
    pub memory_limit: u64,
    /// The number of static tests to run for submission evaluation.
    #[oai(default = "tests_default", validator(maximum(value = "20")))]
    pub static_tests: u8,
    /// The number of random tests to run for submission evaluation.
    #[oai(
        default = "tests_default",
        validator(minimum(value = "1"), maximum(value = "20"))
    )]
    pub random_tests: u8,
    /// The program used to generate test cases and evaluate solutions
    #[oai(validator(max_length = 65536))]
    pub evaluator: String,
    /// The environment to run the solution in.
    pub solution_environment: String,
    /// The solution code
    #[oai(validator(max_length = 65536))]
    pub solution_code: String,
}
fn tests_default() -> u8 {
    10
}

#[derive(Debug, Clone, Object)]
pub struct UpdateCodingChallengeRequest {
    #[oai(flatten)]
    pub subtask: UpdateSubtaskRequest,
    /// The challenge description.
    #[oai(validator(max_length = 16384))]
    pub description: PatchValue<String>,
    /// The number of milliseconds the solution may run.
    #[oai(validator(minimum(value = "1")))]
    pub time_limit: PatchValue<u64>,
    /// The number of megabytes of memory the solution may use.
    #[oai(validator(minimum(value = "1")))]
    pub memory_limit: PatchValue<u64>,
    /// The number of static tests to run for submission evaluation.
    #[oai(validator(maximum(value = "20")))]
    pub static_tests: PatchValue<u8>,
    /// The number of random tests to run for submission evaluation.
    #[oai(validator(minimum(value = "1"), maximum(value = "20")))]
    pub random_tests: PatchValue<u8>,
    /// The program used to generate test cases and evaluate solutions
    #[oai(validator(max_length = 65536))]
    pub evaluator: PatchValue<String>,
    /// The environment to run the solution in.
    pub solution_environment: PatchValue<String>,
    /// The solution code
    #[oai(validator(max_length = 65536))]
    pub solution_code: PatchValue<String>,
}

#[derive(Debug, Clone, Object)]
pub struct Submission {
    /// The unique identifier of the submission.
    pub id: Uuid,
    /// The challenge of the submission.
    pub subtask_id: Uuid,
    /// The create of the submission.
    pub creator: Uuid,
    /// The creation timestamp of the submission.
    pub creation_timestamp: DateTime<Utc>,
    /// The environment of the submission.
    pub environment: String,
    /// The evaluation result of the submission.
    pub result: Option<CheckResult<RunSummary>>,
    /// The number of submissions in the judge's queue before this one.
    pub queue_position: Option<usize>,
}

#[derive(Debug, Clone, Object)]
pub struct SubmissionContent {
    /// The environment to run the solution in.
    pub environment: String,
    /// The solution code.
    #[oai(validator(max_length = 65536))]
    pub code: String,
}

#[derive(Debug, Clone, Object)]
pub struct EvaluatorError {
    /// The exit code of the evaluator.
    pub code: i32,
    /// stderr output of the evaluator.
    pub stderr: String,
}

#[derive(Debug, Clone, Object, Deserialize)]
pub struct RunSummary {
    /// The exit code of the processes.
    pub status: i32,
    /// The stderr output the process produced.
    pub stderr: String,
    /// The amount of resources the process used.
    pub resource_usage: ResourceUsage,
}

#[derive(Debug, Clone, Object, Serialize, Deserialize)]
pub struct CheckResult<T: Send + Sync + Type + ParseFromJSON + ToJSON> {
    pub verdict: ChallengesVerdict,
    pub reason: Option<String>,
    pub compile: Option<T>,
    pub run: Option<T>,
}

#[derive(Debug, Clone, Object)]
pub struct ExecutorConfig {
    /// The maximum `time_limit` in milliseconds.
    pub time_limit: u64,
    /// The maximum `memory_limit` in megabytes.
    pub memory_limit: u64,
}

impl CodingChallengeSummary {
    pub fn from(cc: challenges_coding_challenges::Model, subtask: Subtask) -> Self {
        Self {
            description: subtask.unlocked.then_some(cc.description),
            time_limit: cc.time_limit as _,
            memory_limit: cc.memory_limit as _,
            static_tests: cc.static_tests as _,
            random_tests: cc.random_tests as _,
            subtask,
        }
    }
}

impl CodingChallenge {
    pub fn from(cc: challenges_coding_challenges::Model, subtask: Subtask) -> Self {
        Self {
            description: cc.description,
            time_limit: cc.time_limit as _,
            memory_limit: cc.memory_limit as _,
            static_tests: cc.static_tests as _,
            random_tests: cc.random_tests as _,
            subtask,
        }
    }
}

impl From<RunResult> for RunSummary {
    fn from(value: RunResult) -> Self {
        Self {
            status: value.status,
            stderr: value.stderr,
            resource_usage: value.resource_usage,
        }
    }
}

impl From<CheckResult<RunResult>> for CheckResult<RunSummary> {
    fn from(value: CheckResult<RunResult>) -> Self {
        Self {
            verdict: value.verdict,
            reason: value.reason,
            compile: value.compile.map(Into::into),
            run: value.run.map(Into::into),
        }
    }
}

impl Submission {
    pub fn from(
        submission: challenges_coding_challenge_submissions::Model,
        result: Option<CheckResult<RunSummary>>,
        queue_position: Option<usize>,
    ) -> Self {
        Self {
            id: submission.id,
            subtask_id: submission.subtask_id,
            creator: submission.creator,
            creation_timestamp: submission.creation_timestamp.and_utc(),
            environment: submission.environment,
            result,
            queue_position,
        }
    }
}

impl From<challenges_coding_challenge_result::Model> for CheckResult<RunSummary> {
    fn from(value: challenges_coding_challenge_result::Model) -> Self {
        let summary = |status, stderr, time, memory| {
            Some(RunSummary {
                status: status?,
                stderr: stderr?,
                resource_usage: ResourceUsage {
                    time: time? as _,
                    memory: memory? as _,
                },
            })
        };
        Self {
            verdict: value.verdict,
            reason: value.reason,
            compile: summary(
                value.build_status,
                value.build_stderr,
                value.build_time,
                value.build_memory,
            ),
            run: summary(
                value.run_status,
                value.run_stderr,
                value.run_time,
                value.run_memory,
            ),
        }
    }
}

impl From<PublicConfig> for ExecutorConfig {
    fn from(value: PublicConfig) -> Self {
        Self {
            time_limit: (value.run_limits.time - 1) * 1000,
            memory_limit: value.run_limits.memory,
        }
    }
}
