use std::sync::Arc;

use fnct::format::JsonFormatter;
use lib::{config::Config, SharedState};
use poem_openapi::OpenApi;
use sandkasten_client::SandkastenClient;
use tokio::sync::Semaphore;

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
    /// Endpoints for global challenges (tasks)
    Challenges,
    /// Endpoints for tasks that exist within a course (tasks)
    CourseTasks,
    /// Endpoints for multiple choice subtasks (subtasks)
    MultipleChoice,
    /// Endpoints for coding challenges (subtasks)
    CodingChallenges,
}

pub fn get_api(state: Arc<SharedState>, config: Arc<Config>) -> impl OpenApi {
    (
        Challenges {
            state: state.clone(),
        },
        CourseTasks {
            state: state.clone(),
        },
        MultipleChoice {
            state: state.clone(),
            config: Arc::clone(&config),
        },
        CodingChallenges {
            judge_cache: state.cache.with_formatter(JsonFormatter),
            state,
            sandkasten: SandkastenClient::new(
                config.challenges.coding_challenges.sandkasten_url.clone(),
            ),
            judge_lock: Arc::new(Semaphore::new(
                config.challenges.coding_challenges.max_concurrency,
            )),
        },
    )
}
