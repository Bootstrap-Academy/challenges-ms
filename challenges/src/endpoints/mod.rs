use std::sync::Arc;

use lib::SharedState;
use poem_openapi::OpenApi;

use self::{
    challenges::Challenges, coding_challenges::CodingChallenges, multiple_choice::MultipleChoice,
    skill_tasks::SkillTasks,
};

mod challenges;
mod coding_challenges;
mod multiple_choice;
mod skill_tasks;

#[derive(poem_openapi::Tags)]
pub enum Tags {
    /// Endpoints for global challenges
    Challenges,
    /// Endpoints for tasks that exist within a skill
    SkillTasks,
    /// Endpoints for multiple choice subtasks
    MultipleChoice,
    /// Endpoints for coding challenges
    CodingChallenges,
}

pub fn get_api(state: Arc<SharedState>) -> impl OpenApi {
    (
        Challenges {
            state: state.clone(),
        },
        SkillTasks {
            state: state.clone(),
        },
        MultipleChoice {
            state: state.clone(),
        },
        CodingChallenges { state },
    )
}
