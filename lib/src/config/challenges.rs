use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ChallengesConfig {
    pub host: String,
    pub port: u16,
    pub server: String,
    pub multiple_choice_questions: MultipleChoiceQuestions,
}

#[derive(Debug, Deserialize)]
pub struct MultipleChoiceQuestions {
    pub timeout_incr: u64,
}
