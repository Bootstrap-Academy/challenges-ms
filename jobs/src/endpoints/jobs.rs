use std::collections::HashMap;

use chrono::Utc;
use entity::{jobs_companies, jobs_jobs, jobs_skill_requirements};
use lib::auth::AdminAuth;
use poem_ext::{response, responses::internal_server_error};
use poem_openapi::{payload::Json, OpenApi};
use sea_orm::{ActiveModelTrait, DatabaseConnection, DbErr, EntityTrait, Set};
use uuid::Uuid;

use super::Tags;
use crate::schemas::{
    companies::Company,
    jobs::{CreateJobRequest, Job},
};

pub struct Jobs {
    pub db: DatabaseConnection,
}

#[OpenApi(tag = "Tags::Jobs")]
impl Jobs {
    #[oai(path = "/jobs", method = "get")]
    async fn list_jobs(&self) -> ListJobs::Response {
        let companies = jobs_companies::Entity::find()
            .all(&self.db)
            .await
            .map_err(internal_server_error)?
            .into_iter()
            .map(|company| (company.id, company.into()))
            .collect::<HashMap<Uuid, Company>>();

        let jobs = jobs_jobs::Entity::find()
            .find_with_related(jobs_skill_requirements::Entity)
            .all(&self.db)
            .await
            .map_err(internal_server_error)?
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
                internal_server_error(DbErr::RecordNotFound("Job -> Company".to_owned()))
            })?;

        ListJobs::ok(jobs)
    }

    #[oai(path = "/jobs", method = "post")]
    async fn create_job(
        &self,
        data: Json<CreateJobRequest>,
        _auth: AdminAuth,
    ) -> CreateJob::Response<AdminAuth> {
        let Json(data) = data;
        let company = match jobs_companies::Entity::find_by_id(data.company_id)
            .one(&self.db)
            .await
            .map_err(internal_server_error)?
        {
            Some(company) => company,
            None => {
                return CreateJob::company_not_found();
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
        .map_err(internal_server_error)?;

        jobs_skill_requirements::Entity::insert_many(data.skill_requirements.iter().map(|sr| {
            jobs_skill_requirements::ActiveModel {
                job_id: Set(job.id),
                skill_id: Set(sr.skill_id.clone()),
                level: Set(sr.level),
            }
        }))
        .exec(&self.db)
        .await
        .map_err(internal_server_error)?;

        CreateJob::ok(Job::from(job, company.into(), data.skill_requirements))
    }
}

response!(ListJobs = {
    Ok(200) => Vec<Job>,
});

response!(CreateJob = {
    /// Job has been created successfully
    Ok(201) => Job,
    /// Company does not exist
    CompanyNotFound(404, error),
});
