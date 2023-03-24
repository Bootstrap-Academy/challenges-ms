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
    /// The question text.
    #[oai(validator(max_length = 4096))]
    pub question: String,
    /// The possible answers to the question.
    #[oai(validator(min_items = 1, max_items = 32))]
    pub answers: Vec<Answer>,
    /// The number of xp a user gets for completing this subtask.
    pub xp: i64,
    /// The number of morphcoins a user gets for completing this subtask.
    pub coins: i64,
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
    #[oai(validator(max_length = 4096))]
    pub question: PatchValue<String>,
    /// The possible answers to the question.
    #[oai(validator(min_items = 1, max_items = 32))]
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
            answers: combine_answers(mcq.answers, mcq.correct_answers),
        }
    }
}

impl MultipleChoiceQuestion<String> {
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
            answers: mcq.answers,
        }
    }
}

pub fn combine_answers(answers: Vec<String>, correct: i64) -> Vec<Answer> {
    answers
        .into_iter()
        .enumerate()
        .map(|(i, answer)| Answer {
            answer,
            correct: correct & (1 << i) != 0,
        })
        .collect()
}

pub fn split_answers(answers: Vec<Answer>) -> (Vec<String>, i64) {
    let mut out = Vec::with_capacity(answers.len());
    let correct = answers.into_iter().enumerate().fold(0, |acc, (i, e)| {
        out.push(e.answer);
        acc | ((e.correct as i64) << i)
    });
    (out, correct)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_combine_answers() {
        let answers = vec!["foo".into(), "bar".into(), "baz".into()];
        let correct = 0b011;
        let res = combine_answers(answers, correct);
        assert_eq!(res[0].answer, "foo");
        assert_eq!(res[1].answer, "bar");
        assert_eq!(res[2].answer, "baz");
        assert!(res[0].correct);
        assert!(res[1].correct);
        assert!(!res[2].correct);
    }

    #[test]
    fn test_split_answers() {
        let answers = vec![
            Answer {
                answer: "foo".into(),
                correct: true,
            },
            Answer {
                answer: "bar".into(),
                correct: true,
            },
            Answer {
                answer: "baz".into(),
                correct: false,
            },
        ];
        let (answers, correct) = split_answers(answers);
        assert_eq!(answers, ["foo", "bar", "baz"]);
        assert_eq!(correct, 0b011);
    }
}
