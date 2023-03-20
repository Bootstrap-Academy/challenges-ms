use std::sync::Arc;

use lib::SharedState;
use poem_openapi::OpenApi;

use self::{companies::Companies, jobs::Jobs};

mod companies;
mod jobs;

#[derive(poem_openapi::Tags)]
pub enum Tags {
    /// Endpoints related to companies
    Companies,
    /// Endpoints related to jobs
    Jobs,
}

pub fn get_api(state: Arc<SharedState>) -> impl OpenApi {
    (
        Companies {
            state: state.clone(),
        },
        Jobs { state },
    )
}
