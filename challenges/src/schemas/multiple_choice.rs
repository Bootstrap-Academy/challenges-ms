use chrono::{DateTime, Utc};
use entity::{challenges_multiple_choice_quizes, challenges_subtasks};
use poem_ext::patch_value::PatchValue;
use poem_openapi::{
    types::{ParseFromJSON, ToJSON, Type},
    Object,
};
use uuid::Uuid;

#[derive(Debug, Clone, Object)]
pub struct MultipleChoiceQuestion<A>
where
    A: Send + Sync + Type + ToJSON + ParseFromJSON,
{
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
    /// The question text.
    pub question: String,
    /// The possible answers to the question.
    pub answers: Vec<A>,
}

#[derive(Debug, Clone, Object)]
pub struct CreateMultipleChoiceQuestionRequest {
    #[oai(validator(max_length = 4096))]
    pub question: String,
    #[oai(validator(max_items = 32))]
    pub answers: Vec<Answer>,
}

#[derive(Debug, Clone, Object)]
pub struct UpdateMultipleChoiceQuestionRequest {
    /// The parent task.
    pub task_id: PatchValue<Uuid>,
    /// The number of xp a user gets for completing this subtask.
    pub xp: PatchValue<i64>,
    /// The number of morphcoins a user gets for completing this subtask.
    pub coins: PatchValue<i64>,
    /// The question text.
    pub question: PatchValue<String>,
    /// The possible answers to the question.
    pub answers: PatchValue<Vec<Answer>>,
}

#[derive(Debug, Clone, Object)]
pub struct Answer {
    /// The answer.
    #[oai(validator(max_length = 256))]
    pub answer: String,
    /// Whether this answer is correct.
    pub correct: bool,
}

impl MultipleChoiceQuestion<Answer> {
    pub fn from(
        mcq: challenges_multiple_choice_quizes::Model,
        subtask: challenges_subtasks::Model,
    ) -> Self {
        Self {
            id: subtask.id,
            task_id: subtask.task_id,
            creator: subtask.creator,
            creation_timestamp: subtask.creation_timestamp.and_local_timezone(Utc).unwrap(),
            xp: subtask.xp,
            coins: subtask.coins,
            question: mcq.question,
            answers: mcq
                .answers
                .into_iter()
                .enumerate()
                .map(|(i, answer)| Answer {
                    answer,
                    correct: mcq.correct_answers & (1 << i) != 0,
                })
                .collect(),
        }
    }
}
