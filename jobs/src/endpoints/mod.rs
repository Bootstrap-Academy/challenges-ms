use poem_openapi::OpenApi;
use sea_orm::DatabaseConnection;

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

pub fn get_api(db: DatabaseConnection) -> impl OpenApi {
    (Companies { db: db.clone() }, Jobs { db })
}
