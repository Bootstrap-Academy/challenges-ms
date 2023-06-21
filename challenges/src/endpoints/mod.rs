use std::sync::Arc;

use fnct::format::JsonFormatter;
use lib::{config::Config, SharedState};
use poem_openapi::OpenApi;
use sandkasten_client::SandkastenClient;
use tokio::sync::Semaphore;

use self::{
    challenges::Challenges, coding_challenges::CodingChallenges, course_tasks::CourseTasks,
    multiple_choice::MultipleChoice, subtasks::Subtasks,
};

mod challenges;
mod coding_challenges;
mod course_tasks;
mod multiple_choice;
mod subtasks;

#[derive(poem_openapi::Tags)]
pub enum Tags {
    /// Endpoints for global challenges (tasks)
    Challenges,
    /// Endpoints for tasks that exist within a course (tasks)
    CourseTasks,
    /// Endpoints for all subtasks
    Subtasks,
    /// Endpoints for single/multiple choice questions (subtasks)
    MultipleChoice,
    /// Endpoints for coding challenges (subtasks)
    CodingChallenges,
}

pub fn get_api(
    state: Arc<SharedState>,
    config: Arc<Config>,
    sandkasten: SandkastenClient,
) -> impl OpenApi {
    (
        Challenges {
            state: Arc::clone(&state),
        },
        CourseTasks {
            state: Arc::clone(&state),
            config: Arc::clone(&config),
        },
        Subtasks {
            state: Arc::clone(&state),
            config: Arc::clone(&config),
        },
        MultipleChoice {
            state: Arc::clone(&state),
            config: Arc::clone(&config),
        },
        CodingChallenges {
            judge_cache: state.cache.with_formatter(JsonFormatter),
            state,
            sandkasten,
            judge_lock: Arc::new(Semaphore::new(
                config.challenges.coding_challenges.max_concurrency,
            )),
            config,
        }
        .get_api(),
    )
}
