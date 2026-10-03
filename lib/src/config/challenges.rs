use serde::Deserialize;
use url::Url;

use super::Sentry;

#[derive(Debug, Deserialize)]
pub struct ChallengesConfig {
    pub host: String,
    pub port: u16,
    pub server: String,
    pub sentry: Option<Sentry>,
    pub quizzes: Quizzes, // course tasks
    pub multiple_choice_questions: MultipleChoiceQuestions,
    pub questions: Questions,
    pub matchings: Matchings,
    pub coding_challenges: CodingChallenges,
}

#[derive(Debug, Deserialize)]
pub struct Quizzes {
    pub min_level: u32,
    pub max_xp: u64,
    pub max_coins: u64,
    pub ban_days: Vec<u32>,
}

#[derive(Debug, Deserialize)]
pub struct MultipleChoiceQuestions {
    pub timeout: u64,
    pub hearts: u32,
    pub creator_coins: u32,
}

#[derive(Debug, Deserialize)]
pub struct Questions {
    pub timeout: u64,
    pub hearts: u32,
    pub creator_coins: u32,
}

#[derive(Debug, Deserialize)]
pub struct Matchings {
    pub timeout: u64,
    pub hearts: u32,
    pub creator_coins: u32,
}

#[derive(Debug, Deserialize)]
pub struct CodingChallenges {
    pub sandkasten_url: Url,
    pub max_concurrency: usize,
    pub timeout: u64,
    pub hearts: u32,
    pub creator_coins: u32,
    #[serde(default)]
    pub execution: CodingExecution,
}

/// Operational controls; existing configurations keep the combined API/worker start.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct CodingExecution {
    pub embedded_worker: bool,
    pub max_pending: u32,
    pub max_pending_per_user: u32,
    pub lease_seconds: u32,
    pub poll_milliseconds: u32,
    pub retry_seconds: u32,
    pub max_execution_seconds: u32,
}

impl Default for CodingExecution {
    fn default() -> Self {
        Self {
            embedded_worker: true,
            max_pending: 1024,
            max_pending_per_user: 4,
            lease_seconds: 30,
            poll_milliseconds: 500,
            retry_seconds: 10,
            max_execution_seconds: 600,
        }
    }
}
