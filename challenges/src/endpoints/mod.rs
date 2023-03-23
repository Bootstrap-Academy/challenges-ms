use std::sync::Arc;

use lib::SharedState;
use poem_openapi::OpenApi;

use self::{
    challenges::Challenges, coding_challenges::CodingChallenges, course_tasks::CourseTasks,
    multiple_choice::MultipleChoice,
};

mod challenges;
mod coding_challenges;
mod course_tasks;
mod multiple_choice;

#[derive(poem_openapi::Tags)]
pub enum Tags {
    /// Endpoints for global challenges
    Challenges,
    /// Endpoints for tasks that exist within a course
    CourseTasks,
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
        CourseTasks {
            state: state.clone(),
        },
        MultipleChoice {
            state: state.clone(),
        },
        CodingChallenges { state },
    )
}
