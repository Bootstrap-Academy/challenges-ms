use std::collections::HashMap;

use chrono::Utc;
use entity::{jobs_companies, jobs_jobs, jobs_skill_requirements};
use lib::{auth::AdminAuth, types::Response};
use poem::error::InternalServerError;
use poem_openapi::{payload::Json, ApiResponse, OpenApi};
use sea_orm::{ActiveModelTrait, DatabaseConnection, DbErr, EntityTrait, Set};
use uuid::Uuid;

use super::Tags;
use crate::schemas::{
    companies::Company,
    jobs::{CreateJob, Job},
};

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

    #[oai(path = "/jobs", method = "post")]
    async fn create_job(
        &self,
        data: Json<CreateJob>,
        _auth: AdminAuth,
    ) -> Response<AdminAuth, CreateResponse> {
        let Json(data) = data;
        let company = match jobs_companies::Entity::find_by_id(data.company_id)
            .one(&self.db)
            .await
            .map_err(InternalServerError)?
        {
            Some(company) => company,
            None => {
                return Ok(CreateResponse::CompanyNotFound.into());
            }
        };
        let job = jobs_jobs::ActiveModel {
            id: Set(Uuid::new_v4()),
            company_id: Set(company.id),
            title: Set(data.title),
            description: Set(data.description),
            location: Set(data.location),
            remote: Set(data.remote),
            job_type: Set(data.job_type),
            responsibilities: Set(data.responsibilities),
            professional_level: Set(data.professional_level),
            salary_min: Set(data.salary.min),
            salary_max: Set(data.salary.max),
            salary_unit: Set(data.salary.unit),
            salary_per: Set(data.salary.per),
            contact: Set(data.contact),
            last_update: Set(Utc::now().naive_utc()),
        }
        .insert(&self.db)
        .await
        .map_err(InternalServerError)?;

        jobs_skill_requirements::Entity::insert_many(data.skill_requirements.iter().map(|sr| {
            jobs_skill_requirements::ActiveModel {
                job_id: Set(job.id),
                skill_id: Set(sr.skill_id.clone()),
                level: Set(sr.level),
            }
        }))
        .exec(&self.db)
        .await
        .map_err(InternalServerError)?;

        Ok(CreateResponse::Ok(Json(
            Job::from(job, company.into(), data.skill_requirements).into(),
        ))
        .into())
    }
}

#[derive(ApiResponse)]
enum CreateResponse {
    /// Job has been created successfully
    #[oai(status = 201)]
    Ok(Json<Box<Job>>),
    /// Company does not exist
    #[oai(status = 404)]
    CompanyNotFound,
}
