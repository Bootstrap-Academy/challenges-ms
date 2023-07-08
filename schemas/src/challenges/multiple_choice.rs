use entity::challenges_multiple_choice_quizes;
use poem_ext::patch_value::PatchValue;
use poem_openapi::{
    types::{ParseFromJSON, ToJSON, Type},
    Object,
};

use super::subtasks::{CreateSubtaskRequest, Subtask, UpdateSubtaskRequest};

#[derive(Debug, Clone, Object)]
pub struct MultipleChoiceQuestionSummary {
    #[oai(flatten)]
    pub subtask: Subtask,
    /// The question text. Only available if the user has unlocked the subtask.
    pub question: Option<String>,
    /// Whether this question is a single choice question (exactly one answer is
    /// correct).
    pub single_choice: bool,
}

#[derive(Debug, Clone, Object)]
pub struct MultipleChoiceQuestion<A>
where
    A: Send + Sync + Type + ToJSON + ParseFromJSON,
{
    #[oai(flatten)]
    pub subtask: Subtask,
    /// The question text.
    pub question: String,
    /// The possible answers to the question.
    pub answers: Vec<A>,
    /// Whether this question is a single choice question (exactly one answer is
    /// correct).
    pub single_choice: bool,
}

#[derive(Debug, Clone, Object)]
pub struct CreateMultipleChoiceQuestionRequest {
    #[oai(flatten)]
    pub subtask: CreateSubtaskRequest,
    /// The question text.
    #[oai(validator(max_length = 4096))]
    pub question: String,
    /// The possible answers to the question.
    #[oai(validator(min_items = 1, max_items = 32))]
    pub answers: Vec<Answer>,
    /// Whether this question is a single choice question (exactly one answer is
    /// correct).
    pub single_choice: bool,
}

#[derive(Debug, Clone, Object)]
pub struct UpdateMultipleChoiceQuestionRequest {
    #[oai(flatten)]
    pub subtask: UpdateSubtaskRequest,
    /// The question text.
    #[oai(validator(max_length = 4096))]
    pub question: PatchValue<String>,
    /// The possible answers to the question.
    #[oai(validator(min_items = 1, max_items = 32))]
    pub answers: PatchValue<Vec<Answer>>,
    /// Whether this question is a single choice question (exactly one answer is
    /// correct).
    pub single_choice: PatchValue<bool>,
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
pub struct SolveMCQRequest {
    /// For each possible answer exactly one boolean (`true` for "answer is
    /// correct" or `false` for "answer is incorrect").
    pub answers: Vec<bool>,
}

#[derive(Debug, Clone, Object)]
pub struct SolveMCQFeedback {
    /// Whether the user has successfully solved the question.
    pub solved: bool,
    /// The number of answers that were marked correctly.
    pub correct: usize,
}

impl MultipleChoiceQuestionSummary {
    pub fn from(mcq: challenges_multiple_choice_quizes::Model, subtask: Subtask) -> Self {
        Self {
            question: subtask.unlocked.then_some(mcq.question),
            single_choice: mcq.single_choice,
            subtask,
        }
    }
}

impl MultipleChoiceQuestion<Answer> {
    pub fn from(mcq: challenges_multiple_choice_quizes::Model, subtask: Subtask) -> Self {
        Self {
            question: mcq.question,
            answers: combine_answers(mcq.answers, mcq.correct_answers),
            single_choice: mcq.single_choice,
            subtask,
        }
    }
}

impl MultipleChoiceQuestion<String> {
    pub fn from(mcq: challenges_multiple_choice_quizes::Model, subtask: Subtask) -> Self {
        Self {
            question: mcq.question,
            answers: mcq.answers,
            single_choice: mcq.single_choice,
            subtask,
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
