use chrono::{DateTime, Utc};
use entity::{challenges_questions, challenges_subtasks};
use poem_ext::patch_value::PatchValue;
use poem_openapi::Object;
use uuid::Uuid;

macro_rules! question {
    ($($name:ident(question: $q_ty:ty = $q_val:expr $(, answers: $ans_ty:ty = $ans_val:expr)?);)*) => {
        $(
            #[derive(Debug, Clone, Object)]
            pub struct $name {
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
                /// Whether the user has rated this subtask.
                pub rated: bool,
                /// Whether the subtask is enabled and visible to normal users.
                pub enabled: bool,
                /// The question text.
                pub question: $q_ty,
                $(
                    /// The possible answers to the question.
                    pub answers: $ans_ty,
                )?
                /// Whether the answer is case sensitive.
                pub case_sensitive: bool,
                /// Whether the answer can contain letters.
                pub ascii_letters: bool,
                /// Whether the answer can contain digits.
                pub digits: bool,
                /// Whether the answer can contain symbols like +-*/.,:;_
                pub punctuation: bool,
            }

            impl $name {
                pub fn from(
                    question: challenges_questions::Model,
                    subtask: challenges_subtasks::Model,
                    unlocked: bool,
                    solved: bool,
                    rated: bool,
                ) -> Self {
                    Self {
                        id: subtask.id,
                        task_id: subtask.task_id,
                        creator: subtask.creator,
                        creation_timestamp: subtask.creation_timestamp.and_utc(),
                        xp: subtask.xp as _,
                        coins: subtask.coins as _,
                        fee: subtask.fee as _,
                        unlocked,
                        solved,
                        rated,
                        enabled: subtask.enabled,
                        question: $q_val(question.question, unlocked),
                        $(answers: $ans_val(question.answers),)?
                        case_sensitive: question.case_sensitive,
                        ascii_letters: question.ascii_letters,
                        digits: question.digits,
                        punctuation: question.punctuation,
                    }
                }
            }
        )*
    };
}

question! {
    QuestionSummary(question: Option<String> = |q, u: bool| u.then_some(q));
    Question(question: String = |q, _| q);
    QuestionWithSolution(question: String = |q, _| q, answers: Vec<String> = |a| a);
}

#[derive(Debug, Clone, Object)]
pub struct CreateQuestionRequest {
    /// The number of xp a user gets for completing this subtask.
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub xp: u64,
    /// The number of morphcoins a user gets for completing this subtask.
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub coins: u64,
    /// The number of morphcoins a user has to pay to access this subtask.
    #[oai(validator(maximum(value = "9223372036854775807")), default)]
    pub fee: u64,
    /// The question text.
    pub question: String,
    /// The possible answers to the question.
    pub answers: Vec<String>,
    /// Whether the answer is case sensitive.
    pub case_sensitive: bool,
    /// Whether the answer can contain letters.
    pub ascii_letters: bool,
    /// Whether the answer can contain digits.
    pub digits: bool,
    /// Whether the answer can contain puncutation characters
    pub punctuation: bool,
}

#[derive(Debug, Clone, Object)]
pub struct UpdateQuestionRequest {
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
    /// Whether the subtask is enabled and visible to normal users.
    pub enabled: PatchValue<bool>,
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
