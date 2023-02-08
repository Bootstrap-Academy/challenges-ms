use poem_openapi::OpenApi;
use sea_orm::DatabaseConnection;

use self::companies::Companies;

mod companies;

#[derive(poem_openapi::Tags)]
pub enum Tags {
    /// Endpoints related to companies
    Companies,
}

pub fn get_api(db: DatabaseConnection) -> impl OpenApi {
    Companies { db }
}
