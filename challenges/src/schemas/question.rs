use entity::challenges_questions;
use poem_ext::patch_value::PatchValue;
use poem_openapi::Object;

use super::subtasks::{CreateSubtaskRequest, Subtask, UpdateSubtaskRequest};

#[derive(Debug, Clone, Object)]
pub struct QuestionSummary {
    #[oai(flatten)]
    pub subtask: Subtask,
    // The question text.
    pub question: Option<String>,
    // Whether the answer is case sensitive.
    pub case_sensitive: bool,
    // Whether the answer can contain letters.
    pub ascii_letters: bool,
    // Whether the answer can contain digits.
    pub digits: bool,
    // Whether the answer can contain symbols like +-*/.,:;_
    pub punctuation: bool,
    // The list of \"building blocks\" that can be used to compose the answer.
    // Empty if the answer has to be typed.
    pub blocks: Vec<String>,
}

#[derive(Debug, Clone, Object)]
pub struct Question {
    #[oai(flatten)]
    pub subtask: Subtask,
    // The question text.
    pub question: String,
    // Whether the answer is case sensitive.
    pub case_sensitive: bool,
    // Whether the answer can contain letters.
    pub ascii_letters: bool,
    // Whether the answer can contain digits.
    pub digits: bool,
    // Whether the answer can contain symbols like +-*/.,:;_
    pub punctuation: bool,
    // The list of \"building blocks\" that can be used to compose the answer.
    // Empty if the answer has to be typed.
    pub blocks: Vec<String>,
}

#[derive(Debug, Clone, Object)]
pub struct QuestionWithSolution {
    #[oai(flatten)]
    pub subtask: Subtask,
    // The question text.
    pub question: String,
    // The possible answers to the question.
    pub answers: Vec<String>,
    // Whether the answer is case sensitive.
    pub case_sensitive: bool,
    // Whether the answer can contain letters.
    pub ascii_letters: bool,
    // Whether the answer can contain digits.
    pub digits: bool,
    // Whether the answer can contain symbols like +-*/.,:;_
    pub punctuation: bool,
    // The list of \"building blocks\" that can be used to compose the answer.
    // Empty if the answer has to be typed.
    pub blocks: Vec<String>,
}

#[derive(Debug, Clone, Object)]
pub struct CreateQuestionRequest {
    #[oai(flatten)]
    pub subtask: CreateSubtaskRequest,
    /// The question text.
    #[oai(validator(max_length = 4096))]
    pub question: String,
    /// The possible answers to the question.
    #[oai(validator(min_items = 1, max_items = 32, max_length = 256))]
    pub answers: Vec<String>,
    /// Whether the answer is case sensitive.
    pub case_sensitive: bool,
    /// Whether the answer can contain letters.
    pub ascii_letters: bool,
    /// Whether the answer can contain digits.
    pub digits: bool,
    /// Whether the answer can contain puncutation characters
    pub punctuation: bool,
    /// The list of "building blocks" that can be used to compose the answer.
    /// Empty if the answer has to be typed.
    #[oai(validator(max_items = 32, max_length = 256))]
    pub blocks: Vec<String>,
}

#[derive(Debug, Clone, Object)]
pub struct UpdateQuestionRequest {
    #[oai(flatten)]
    pub subtask: UpdateSubtaskRequest,
    /// The question text.
    #[oai(validator(max_length = 4096))]
    pub question: PatchValue<String>,
    /// The possible answers to the question.
    #[oai(validator(min_items = 1, max_items = 32, max_length = 256))]
    pub answers: PatchValue<Vec<String>>,
    /// Whether the answer is case sensitive.
    pub case_sensitive: PatchValue<bool>,
    /// Whether the answer can contain letters.
    pub ascii_letters: PatchValue<bool>,
    /// Whether the answer can contain digits.
    pub digits: PatchValue<bool>,
    /// Whether the answer can contain puncutation characters
    pub punctuation: PatchValue<bool>,
    /// The list of "building blocks" that can be used to compose the answer.
    /// Empty if the answer has to be typed.
    #[oai(validator(max_items = 32, max_length = 256))]
    pub blocks: PatchValue<Vec<String>>,
}

#[derive(Debug, Clone, Object)]
pub struct SolveQuestionRequest {
    pub answer: String,
}

#[derive(Debug, Clone, Object)]
pub struct SolveQuestionFeedback {
    /// Whether the user has successfully solved the question.
    pub solved: bool,
}

impl QuestionSummary {
    pub fn from(question: challenges_questions::Model, subtask: Subtask) -> Self {
        Self {
            question: subtask.unlocked.then_some(question.question),
            case_sensitive: question.case_sensitive,
            ascii_letters: question.ascii_letters,
            digits: question.digits,
            punctuation: question.punctuation,
            blocks: question.blocks,
            subtask,
        }
    }
}

impl Question {
    pub fn from(question: challenges_questions::Model, subtask: Subtask) -> Self {
        Self {
            question: question.question,
            case_sensitive: question.case_sensitive,
            ascii_letters: question.ascii_letters,
            digits: question.digits,
            punctuation: question.punctuation,
            blocks: question.blocks,
            subtask,
        }
    }
}

impl QuestionWithSolution {
    pub fn from(question: challenges_questions::Model, subtask: Subtask) -> Self {
        Self {
            question: question.question,
            answers: question.answers,
            case_sensitive: question.case_sensitive,
            ascii_letters: question.ascii_letters,
            digits: question.digits,
            punctuation: question.punctuation,
            blocks: question.blocks,
            subtask,
        }
    }
}
