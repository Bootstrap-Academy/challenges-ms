use chrono::{DateTime, Utc};
use entity::{challenges_coding_challenges, challenges_subtasks};
use poem_ext::patch_value::PatchValue;
use poem_openapi::{
    types::{ParseFromJSON, ToJSON, Type},
    Enum, Object,
};
use sandkasten_client::schemas::programs::{Limits, ResourceUsage, RunResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Object)]
pub struct CodingChallenge {
    /// The unique identifier of the subtask.
    pub id: Uuid,
    /// The parent task.
    pub task_id: Uuid,
    /// The creator of the subtask
    pub creator: Uuid,
    /// The creation timestamp of the subtask
    pub creation_timestamp: DateTime<Utc>,
    /// The number of xp a user gets for completing this subtask.
    pub xp: i64,
    /// The number of morphcoins a user gets for completing this subtask.
    pub coins: i64,
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
    /// The number of xp a user gets for completing this subtask.
    pub xp: i64,
    /// The number of morphcoins a user gets for completing this subtask.
    pub coins: i64,
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
    /// The parent task.
    pub task_id: PatchValue<Uuid>,
    /// The number of xp a user gets for completing this subtask.
    pub xp: PatchValue<i64>,
    /// The number of morphcoins a user gets for completing this subtask.
    pub coins: PatchValue<i64>,
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
    /// The environment to run the solution in.
    pub environment: String,
    /// The solution code.
    #[oai(validator(max_length = 65536))]
    pub code: String,
}

#[derive(Debug, Clone, Object)]
pub struct SubmissionResult {
    /// The final verdict of the submission.
    pub verdict: Verdict,
    /// An optional reason for the verdict.
    pub reason: Option<String>,
    /// The stderr output of the compile step.
    pub build_stderr: Option<String>,
    /// The number of milliseconds the build step ran.
    pub build_time: Option<u64>,
    /// The amount of memory the build step used (in KB)
    pub build_memory: Option<u64>,
    /// The stderr output of the run step.
    pub run_stderr: Option<String>,
    /// The number of milliseconds the run step ran.
    pub run_time: Option<u64>,
    /// The amount of memory the run step used (in KB)
    pub run_memory: Option<u64>,
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
    /// The limits that applied to the process.
    pub limits: Limits,
}

#[derive(Debug, Clone, Enum, Serialize, Deserialize)]
#[oai(rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    Ok,
    WrongAnswer,
    InvalidOutputFormat,
    TimeLimitExceeded,
    MemoryLimitExceeded,
    NoOutput,
    CompilationError,
    RuntimeError,
}

#[derive(Debug, Clone, Object, Serialize, Deserialize)]
pub struct CheckResult<T: Send + Sync + Type + ParseFromJSON + ToJSON> {
    pub verdict: Verdict,
    pub reason: Option<String>,
    pub compile: Option<T>,
    pub run: Option<T>,
}

impl CodingChallenge {
    pub fn from(
        cc: challenges_coding_challenges::Model,
        subtask: challenges_subtasks::Model,
    ) -> Self {
        Self {
            id: subtask.id,
            task_id: subtask.task_id,
            creator: subtask.creator,
            creation_timestamp: subtask.creation_timestamp.and_local_timezone(Utc).unwrap(),
            xp: subtask.xp,
            coins: subtask.coins,
            description: cc.description,
            time_limit: cc.time_limit as _,
            memory_limit: cc.memory_limit as _,
            static_tests: cc.static_tests as _,
            random_tests: cc.random_tests as _,
        }
    }
}

impl From<RunResult> for RunSummary {
    fn from(value: RunResult) -> Self {
        Self {
            status: value.status,
            stderr: value.stderr,
            resource_usage: value.resource_usage,
            limits: value.limits,
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
