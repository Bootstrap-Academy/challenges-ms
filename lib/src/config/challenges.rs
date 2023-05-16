use serde::Deserialize;
use url::Url;

#[derive(Debug, Deserialize)]
pub struct ChallengesConfig {
    pub host: String,
    pub port: u16,
    pub server: String,
    pub multiple_choice_questions: MultipleChoiceQuestions,
    pub coding_challenges: CodingChallenges,
}

#[derive(Debug, Deserialize)]
pub struct MultipleChoiceQuestions {
    pub timeout_incr: u64,
}

#[derive(Debug, Deserialize)]
pub struct CodingChallenges {
    pub sandkasten_url: Url,
}
