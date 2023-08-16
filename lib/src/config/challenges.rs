use serde::Deserialize;
use url::Url;

#[derive(Debug, Deserialize)]
pub struct ChallengesConfig {
    pub host: String,
    pub port: u16,
    pub server: String,
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
}
