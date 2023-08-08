use std::sync::Arc;

use fnct::format::JsonFormatter;
use lib::{config::Config, SharedState};
use poem_openapi::OpenApi;
use sandkasten_client::SandkastenClient;
use tokio::sync::Semaphore;

use self::{
    challenges::Challenges, coding_challenges::CodingChallenges, course_tasks::CourseTasks,
    leaderboard::Leaderboard, matchings::Matchings, multiple_choice::MultipleChoice,
    question::Questions, subtasks::Subtasks,
};

mod challenges;
pub mod coding_challenges;
mod course_tasks;
mod leaderboard;
mod matchings;
mod multiple_choice;
mod question;
mod subtasks;

#[derive(poem_openapi::Tags)]
pub enum Tags {
    /// Global challenges (tasks)
    Challenges,
    /// Tasks that exist within a course (tasks)
    CourseTasks,
    /// Endpoints related to all subtasks
    Subtasks,
    /// Single/multiple choice questions (subtasks)
    MultipleChoice,
    /// Simple questions with typed answers (subtasks)
    Questions,
    /// One to one matchings (subtasks)
    Matchings,
    /// Coding challenges (subtasks)
    CodingChallenges,
    /// Leaderboard
    Leaderboard,
}

pub async fn setup_api(
    state: Arc<SharedState>,
    config: Arc<Config>,
    sandkasten: SandkastenClient,
) -> anyhow::Result<impl OpenApi> {
    Ok((
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
        }
        .get_api(),
        MultipleChoice {
            state: Arc::clone(&state),
            config: Arc::clone(&config),
        },
        Questions {
            state: Arc::clone(&state),
            config: Arc::clone(&config),
        },
        Matchings {
            state: Arc::clone(&state),
            config: Arc::clone(&config),
        },
        CodingChallenges {
            judge_cache: state.cache.with_formatter(JsonFormatter),
            state: Arc::clone(&state),
            sandkasten,
            judge_lock: Arc::new(Semaphore::new(
                config.challenges.coding_challenges.max_concurrency,
            )),
            config,
        }
        .setup_api()
        .await?,
        Leaderboard { state },
    ))
}
