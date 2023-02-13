use std::collections::HashMap;

use entity::{jobs_companies, jobs_jobs, jobs_skill_requirements};
use lib::types::Response;
use poem::error::InternalServerError;
use poem_openapi::{payload::Json, OpenApi};
use sea_orm::{DatabaseConnection, DbErr, EntityTrait};
use uuid::Uuid;

use super::Tags;
use crate::schemas::{companies::Company, jobs::Job};

pub struct Jobs {
    pub db: DatabaseConnection,
}

#[OpenApi(tag = "Tags::Jobs")]
impl Jobs {
    #[oai(path = "/jobs", method = "get")]
    async fn list_jobs(&self) -> Response<(), Json<Vec<Job>>> {
        let companies = jobs_companies::Entity::find()
            .all(&self.db)
            .await
            .map_err(InternalServerError)?
            .into_iter()
            .map(|company| (company.id, company.into()))
            .collect::<HashMap<Uuid, Company>>();

        let jobs = jobs_jobs::Entity::find()
            .find_with_related(jobs_skill_requirements::Entity)
            .all(&self.db)
            .await
            .map_err(InternalServerError)?
            .into_iter()
            .map(|(job, skill_requirements)| {
                companies.get(&job.company_id).map(|company| {
                    Job::from(
                        job,
                        company.clone(),
                        skill_requirements.into_iter().map(Into::into).collect(),
                    )
                })
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| {
                InternalServerError(DbErr::RecordNotFound("Job -> Company".to_owned()))
            })?;

        Ok(Json(jobs).into())
    }
}
