use chrono::{DateTime, Utc};
use entity::{challenges_multiple_choice_quizes, challenges_subtasks};
use poem_ext::patch_value::PatchValue;
use poem_openapi::{
    types::{ParseFromJSON, ToJSON, Type},
    Object,
};
use uuid::Uuid;

#[derive(Debug, Clone, Object)]
pub struct MultipleChoiceQuestionSummary {
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
    /// The question text. Only available if the user has unlocked the subtask.
    pub question: Option<String>,
}

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
    pub xp: u64,
    /// The number of morphcoins a user gets for completing this subtask.
    pub coins: u64,
    /// The number of morphcoins a user has to pay to access this subtask.
    pub fee: u64,
    /// Whether the user has completed this subtask.
    pub solved: bool,
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
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub xp: u64,
    /// The number of morphcoins a user gets for completing this subtask.
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub coins: u64,
    /// The number of morphcoins a user has to pay to access this subtask.
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub fee: u64,
}

#[derive(Debug, Clone, Object)]
pub struct UpdateMultipleChoiceQuestionRequest {
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

#[derive(Debug, Clone, Object)]
pub struct SolveQuestionRequest {
    /// For each possible answer exactly one boolean (`true` for "answer is
    /// correct" or `false` for "answer is incorrect").
    pub answers: Vec<bool>,
}

#[derive(Debug, Clone, Object)]
pub struct SolveQuestionFeedback {
    /// Whether the user has successfully solved the question.
    pub solved: bool,
    /// The number of answers that were marked correctly.
    pub correct: usize,
}

impl MultipleChoiceQuestionSummary {
    pub fn from(
        mcq: challenges_multiple_choice_quizes::Model,
        subtask: challenges_subtasks::Model,
        unlocked: bool,
        solved: bool,
    ) -> Self {
        Self {
            id: subtask.id,
            task_id: subtask.task_id,
            creator: subtask.creator,
            creation_timestamp: subtask.creation_timestamp.and_local_timezone(Utc).unwrap(),
            xp: subtask.xp as _,
            coins: subtask.coins as _,
            fee: subtask.fee as _,
            unlocked,
            solved,
            question: unlocked.then_some(mcq.question),
        }
    }
}

impl MultipleChoiceQuestion<Answer> {
    pub fn from(
        mcq: challenges_multiple_choice_quizes::Model,
        subtask: challenges_subtasks::Model,
        solved: bool,
    ) -> Self {
        Self {
            id: subtask.id,
            task_id: subtask.task_id,
            creator: subtask.creator,
            creation_timestamp: subtask.creation_timestamp.and_local_timezone(Utc).unwrap(),
            xp: subtask.xp as _,
            coins: subtask.coins as _,
            fee: subtask.fee as _,
            solved,
            question: mcq.question,
            answers: combine_answers(mcq.answers, mcq.correct_answers),
        }
    }
}

impl MultipleChoiceQuestion<String> {
    pub fn from(
        mcq: challenges_multiple_choice_quizes::Model,
        subtask: challenges_subtasks::Model,
        solved: bool,
    ) -> Self {
        Self {
            id: subtask.id,
            task_id: subtask.task_id,
            creator: subtask.creator,
            creation_timestamp: subtask.creation_timestamp.and_local_timezone(Utc).unwrap(),
            xp: subtask.xp as _,
            coins: subtask.coins as _,
            fee: subtask.fee as _,
            solved,
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

pub fn check_answers(answers: &[bool], correct: i64) -> usize {
    answers
        .iter()
        .enumerate()
        .filter(|(i, &answer)| (correct & (1 << i) != 0) == answer)
        .count()
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

    #[test]
    fn test_check_answers() {
        assert_eq!(check_answers(&[true, true, false, true], 0b1001), 3);
        assert_eq!(check_answers(&[true, true, true, true], 0b1001), 2);
        assert_eq!(check_answers(&[true, false, false, true], 0b1001), 4);
        assert_eq!(check_answers(&[true, true, true, false], 0b1001), 1);
        assert_eq!(check_answers(&[false, true, true, false], 0b1001), 0);
    }
}
