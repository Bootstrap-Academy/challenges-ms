use chrono::{DateTime, Utc};
use entity::{
    challenges_coding_challenge_result, challenges_coding_challenge_submissions,
    challenges_coding_challenges, challenges_subtasks, sea_orm_active_enums::ChallengesVerdict,
};
use poem_ext::patch_value::PatchValue;
use poem_openapi::{
    types::{ParseFromJSON, ToJSON, Type},
    Object,
};
use sandkasten_client::schemas::programs::{ResourceUsage, RunResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Object)]
pub struct CodingChallengeSummary {
    /// The unique identifier of the subtask.
    pub id: Uuid,
    /// The parent task.
    pub task_id: Uuid,
    /// The creator of the subtask
    pub creator: Uuid,
    /// The creation timestamp of the subtask
    pub creation_timestamp: DateTime<Utc>,
    /// The number of xp a user gets for completing this subtask.
    pub xp: u64,
    /// The number of morphcoins a user gets for completing this subtask.
    pub coins: u64,
    /// The number of morphcoins a user has to pay to access this subtask.
    pub fee: u64,
    /// Whether the user has unlocked this subtask.
    pub unlocked: bool,
    /// Whether the user has completed this subtask.
    pub solved: bool,
    /// Whether the user has rated this subtask.
    pub rated: bool,
    /// Whether the subtask is enabled and visible to normal users.
    pub enabled: bool,
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
    /// The unique identifier of the subtask.
    pub id: Uuid,
    /// The parent task.
    pub task_id: Uuid,
    /// The creator of the subtask
    pub creator: Uuid,
    /// The creation timestamp of the subtask
    pub creation_timestamp: DateTime<Utc>,
    /// The number of xp a user gets for completing this subtask.
    pub xp: u64,
    /// The number of morphcoins a user gets for completing this subtask.
    pub coins: u64,
    /// The number of morphcoins a user has to pay to access this subtask.
    pub fee: u64,
    /// Whether the user has completed this subtask.
    pub solved: bool,
    /// Whether the user has rated this subtask.
    pub rated: bool,
    /// Whether the subtask is enabled and visible to normal users.
    pub enabled: bool,
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
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub xp: u64,
    /// The number of morphcoins a user gets for completing this subtask.
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub coins: u64,
    /// The number of morphcoins a user has to pay to access this subtask.
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub fee: u64,
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
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub xp: PatchValue<u64>,
    /// The number of morphcoins a user gets for completing this subtask.
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub coins: PatchValue<u64>,
    /// The number of morphcoins a user has to pay to access this subtask.
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub fee: PatchValue<u64>,
    /// Whether the subtask is enabled and visible to normal users.
    pub enabled: PatchValue<bool>,
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
    pub fn from(
        cc: challenges_coding_challenges::Model,
        subtask: challenges_subtasks::Model,
        unlocked: bool,
        solved: bool,
        rated: bool,
    ) -> Self {
        Self {
            id: subtask.id,
            task_id: subtask.task_id,
            creator: subtask.creator,
            creation_timestamp: subtask.creation_timestamp.and_utc(),
            xp: subtask.xp as _,
            coins: subtask.coins as _,
            fee: subtask.fee as _,
            unlocked,
            solved,
            rated,
            enabled: subtask.enabled,
            description: unlocked.then_some(cc.description),
            time_limit: cc.time_limit as _,
            memory_limit: cc.memory_limit as _,
            static_tests: cc.static_tests as _,
            random_tests: cc.random_tests as _,
        }
    }
}

impl CodingChallenge {
    pub fn from(
        cc: challenges_coding_challenges::Model,
        subtask: challenges_subtasks::Model,
        solved: bool,
        rated: bool,
    ) -> Self {
        Self {
            id: subtask.id,
            task_id: subtask.task_id,
            creator: subtask.creator,
            creation_timestamp: subtask.creation_timestamp.and_utc(),
            xp: subtask.xp as _,
            coins: subtask.coins as _,
            fee: subtask.fee as _,
            solved,
            rated,
            enabled: subtask.enabled,
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
    ) -> Self {
        Self {
            id: submission.id,
            subtask_id: submission.subtask_id,
            creator: submission.creator,
            creation_timestamp: submission.creation_timestamp.and_utc(),
            environment: submission.environment,
            result,
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
