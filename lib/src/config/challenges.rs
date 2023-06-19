use serde::Deserialize;
use url::Url;

#[derive(Debug, Deserialize)]
pub struct ChallengesConfig {
    pub host: String,
    pub port: u16,
    pub server: String,
    pub quizzes: Quizzes,
    pub multiple_choice_questions: MultipleChoiceQuestions,
    pub coding_challenges: CodingChallenges,
}

#[derive(Debug, Deserialize)]
pub struct Quizzes {
    pub min_level: u32,
    pub max_xp: u64,
    pub max_coins: u64,
    pub max_fee: u64,
}

#[derive(Debug, Deserialize)]
pub struct MultipleChoiceQuestions {
    pub timeout_incr: u64,
}

#[derive(Debug, Deserialize)]
pub struct CodingChallenges {
    pub sandkasten_url: Url,
    pub max_concurrency: usize,
}
