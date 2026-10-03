//! XP milestones for completed skills-ms lesson units.
//!
//! Only skills-ms reports a milestone, after its own verified completion. A
//! milestone never costs hearts or awards coins.

use chrono::{DateTime, Utc};
use poem_openapi::{Enum, Object};

/// How skills-ms verified the completion of the lesson unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
#[oai(rename_all = "snake_case")]
pub enum LessonMilestoneCompletion {
    /// A deterministic server-side check of the lesson's answer.
    Deterministic,
    /// A passed, gateway-signed LLM verdict.
    LlmVerdict,
}

impl LessonMilestoneCompletion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deterministic => "deterministic",
            Self::LlmVerdict => "llm_verdict",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "deterministic" => Some(Self::Deterministic),
            "llm_verdict" => Some(Self::LlmVerdict),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Object)]
pub struct RecordLessonMilestoneRequest {
    /// The sub-skill that receives the XP.
    #[oai(validator(min_length = 1, max_length = 256))]
    pub skill_id: String,
    /// The XP of this lesson unit, as authored in the skills-ms catalogue.
    #[oai(validator(minimum(value = "1")))]
    pub xp: u64,
    /// How skills-ms verified the completion.
    pub completion: LessonMilestoneCompletion,
}

#[derive(Debug, Clone, Object)]
pub struct LessonMilestone {
    /// The skills-ms unit that was completed.
    pub unit_id: String,
    /// The sub-skill that receives the XP.
    pub skill_id: String,
    /// The XP recorded for this milestone.
    pub xp: u64,
    /// How skills-ms verified the completion.
    pub completion: LessonMilestoneCompletion,
    /// When the milestone was first recorded.
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Object)]
pub struct RecordedLessonMilestone {
    /// Whether this call recorded the milestone. A repeated call returns the
    /// original milestone unchanged and awards nothing.
    pub created: bool,
    /// The milestone as first recorded.
    pub milestone: LessonMilestone,
}
