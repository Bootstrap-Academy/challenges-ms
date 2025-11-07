use entity::{challenges_cloze_blanks, challenges_cloze_options, challenges_clozes};
use poem_ext::patch_value::PatchValue;
use poem_openapi::{Enum, Object};
use uuid::Uuid;

use super::subtasks::{CreateSubtaskRequest, Subtask, UpdateSubtaskRequest};

#[derive(Debug, Clone, Enum)]
#[oai(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClozeVariant {
    /// No options are given; learners must type all answers.
    TypeIn,
    /// Each blank must be matched against a provided option.
    Options,
}

#[derive(Debug, Clone, Object)]
pub struct ClozeSummary {
    #[oai(flatten)]
    pub subtask: Subtask,
    /// The markdown text that contains placeholders like `{{blank_1}}`.
    pub content: String,
    /// Whether comparisons are case sensitive for all blanks in this cloze.
    pub case_sensitive: bool,
    /// The detected variant inferred from the stored options.
    pub variant: ClozeVariant,
    /// Metadata for each blank without revealing the solution.
    pub blanks: Vec<ClozeBlank>,
    /// Optional pool of options (variant B) that can be assigned to blanks.
    pub options: Vec<ClozeOption>,
}

#[derive(Debug, Clone, Object)]
pub struct Cloze {
    #[oai(flatten)]
    pub summary: ClozeSummary,
}

#[derive(Debug, Clone, Object)]
pub struct ClozeWithSolution {
    #[oai(flatten)]
    pub summary: ClozeSummary,
    /// The solution metadata for each blank.
    pub blank_solutions: Vec<ClozeBlankWithSolution>,
}

#[derive(Debug, Clone, Object)]
pub struct ClozeBlank {
    /// Unique identifier of the blank.
    pub id: Uuid,
    /// The 1-based placeholder index extracted from the markdown (`{{blank_<index>}}`).
    pub placeholder: u32,
}

#[derive(Debug, Clone, Object)]
pub struct ClozeBlankWithSolution {
    #[oai(flatten)]
    pub blank: ClozeBlank,
    /// The canonical answer for this blank.
    pub answer: String,
    /// A small list of accepted synonyms (case sensitivity depends on the cloze settings).
    pub synonyms: Vec<String>,
    /// The option id that solves this blank for variant B; `null` for variant A.
    pub option_id: Option<Uuid>,
}

#[derive(Debug, Clone, Object)]
pub struct ClozeOption {
    /// Unique identifier of the option.
    pub id: Uuid,
    /// Visible label of the option.
    pub label: String,
}

#[derive(Debug, Clone, Object)]
pub struct CreateClozeRequest {
    #[oai(flatten)]
    pub subtask: CreateSubtaskRequest,
    /// Markdown text that contains numbered placeholders in the form `{{blank_<index>}}`.
    /// The placeholders define where answers will be rendered.
    #[oai(validator(max_length = 16384))]
    pub content: String,
    /// Ordered list of blank definitions. Each placeholder referenced in `content` must appear here
    /// exactly once. Extra definitions that refer to non-existent placeholders are ignored.
    #[oai(validator(min_items = 1, max_items = 32))]
    pub blanks: Vec<CreateClozeBlank>,
    /// Option pool for variant B. If this list is empty, the cloze behaves like variant A
    /// (learner types the answer). When it is not empty, `len(options)` must be >= number of blanks.
    #[oai(validator(max_items = 64, max_length = 256))]
    pub options: Vec<String>,
    /// Whether comparisons should be case sensitive. Defaults to `false`.
    #[oai(default)]
    pub case_sensitive: bool,
}

#[derive(Debug, Clone, Object, PartialEq, Eq)]
pub struct CreateClozeBlank {
    /// The placeholder index that this definition belongs to (the `<index>` in `{{blank_<index>}}`).
    #[oai(validator(minimum(value = "1"), maximum(value = "2147483647")))]
    pub placeholder: u32,
    /// The canonical answer for this blank.
    #[oai(validator(min_length = 1, max_length = 512))]
    pub answer: String,
    /// Optional synonyms that should be accepted for variant A.
    #[oai(validator(max_items = 8, max_length = 256), default)]
    pub synonyms: Vec<String>,
    /// Index inside the `options` array that solves this blank for variant B. Must be `None`
    /// when `options` is empty.
    pub option_index: Option<u32>,
}

#[derive(Debug, Clone, Object)]
pub struct UpdateClozeRequest {
    #[oai(flatten)]
    pub subtask: UpdateSubtaskRequest,
    /// Updated markdown body.
    #[oai(validator(max_length = 16384))]
    pub content: PatchValue<String>,
    /// Full replacement for all blank definitions. Omit this field to keep the current configuration.
    #[oai(validator(min_items = 1, max_items = 32))]
    pub blanks: PatchValue<Vec<CreateClozeBlank>>,
    /// Full replacement for the options pool. Omit to keep the existing options.
    #[oai(validator(max_items = 64, max_length = 256))]
    pub options: PatchValue<Vec<String>>,
    /// Case sensitivity flag.
    pub case_sensitive: PatchValue<bool>,
}

#[derive(Debug, Clone, Object)]
pub struct SolveClozeRequest {
    /// Exactly one entry per blank id returned by the API.
    #[oai(validator(min_items = 1, max_items = 32))]
    pub answers: Vec<ClozeAnswerSubmission>,
}

#[derive(Debug, Clone, Object)]
pub struct ClozeAnswerSubmission {
    /// The blank identifier to fill.
    pub blank_id: Uuid,
    /// Free-form answer (variant A).
    #[oai(validator(max_length = 512))]
    pub text: Option<String>,
    /// Selected option id (variant B).
    pub option_id: Option<Uuid>,
}

#[derive(Debug, Clone, Object)]
pub struct SolveClozeFeedback {
    /// Whether every blank was correct.
    pub solved: bool,
    /// Number of correctly solved blanks.
    pub correct: u32,
    /// Total number of blanks in this cloze.
    pub total: u32,
}

impl ClozeSummary {
    pub fn from(
        cloze: &challenges_clozes::Model,
        subtask: Subtask,
        blanks: &[challenges_cloze_blanks::Model],
        options: &[challenges_cloze_options::Model],
    ) -> Self {
        Self {
            subtask,
            content: cloze.content.clone(),
            case_sensitive: cloze.case_sensitive,
            variant: if options.is_empty() {
                ClozeVariant::TypeIn
            } else {
                ClozeVariant::Options
            },
            blanks: blanks
                .iter()
                .map(|blank| ClozeBlank {
                    id: blank.id,
                    placeholder: u32::try_from(blank.placeholder).unwrap_or_default(),
                })
                .collect(),
            options: options
                .iter()
                .map(|option| ClozeOption {
                    id: option.id,
                    label: option.label.clone(),
                })
                .collect(),
        }
    }
}

impl Cloze {
    pub fn from(
        cloze: &challenges_clozes::Model,
        subtask: Subtask,
        blanks: &[challenges_cloze_blanks::Model],
        options: &[challenges_cloze_options::Model],
    ) -> Self {
        Self {
            summary: ClozeSummary::from(cloze, subtask, blanks, options),
        }
    }
}

impl ClozeWithSolution {
    pub fn from(
        cloze: &challenges_clozes::Model,
        subtask: Subtask,
        blanks: &[challenges_cloze_blanks::Model],
        options: &[challenges_cloze_options::Model],
    ) -> Self {
        let summary = ClozeSummary::from(cloze, subtask, blanks, options);
        Self {
            blank_solutions: blanks
                .iter()
                .map(|blank| ClozeBlankWithSolution {
                    blank: ClozeBlank {
                        id: blank.id,
                        placeholder: u32::try_from(blank.placeholder).unwrap_or_default(),
                    },
                    answer: blank.answer.clone(),
                    synonyms: blank.synonyms.clone(),
                    option_id: blank.correct_option_id,
                })
                .collect(),
            summary,
        }
    }
}
