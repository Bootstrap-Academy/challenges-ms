use std::{collections::HashMap, sync::Arc};

use chrono::Utc;
use entity::{jobs_companies, jobs_jobs, jobs_skill_requirements};
use itertools::Itertools;
use lib::{auth::AdminAuth, SharedState};
use poem::web::Data;
use poem_ext::{db::DbTxn, response, responses::internal_server_error};
use poem_openapi::{payload::Json, OpenApi};
use sea_orm::{ActiveModelTrait, DbErr, EntityTrait, Set};
use tracing::warn;
use uuid::Uuid;

use super::Tags;
use crate::schemas::{
    companies::Company,
    jobs::{CreateJobRequest, Job, SkillRequirement},
};

pub struct Jobs {
    pub state: Arc<SharedState>,
}

#[OpenApi(tag = "Tags::Jobs")]
impl Jobs {
    #[oai(path = "/jobs", method = "get")]
    async fn list_jobs(&self, db: Data<&DbTxn>) -> ListJobs::Response {
        let companies = jobs_companies::Entity::find()
            .all(&***db)
            .await?
            .into_iter()
            .map(|company| (company.id, company.into()))
            .collect::<HashMap<Uuid, Company>>();

        let skills = self.state.services.skills.get_skills().await?;

        let jobs = jobs_jobs::Entity::find()
            .find_with_related(jobs_skill_requirements::Entity)
            .all(&***db)
            .await?
            .into_iter()
            .map(|(job, skill_requirements)| {
                companies.get(&job.company_id).map(|company| {
                    Job::from(
                        job,
                        company.clone(),
                        skill_requirements
                            .into_iter()
                            .map(|req| {
                                let parent_skill_id = match skills.get(&req.skill_id) {
                                    Some(skill) => Some(skill.parent_id.clone()),
                                    None => {
                                        warn!(
                                            "Could not find parent_id of skill {} in job {}",
                                            req.skill_id, req.job_id
                                        );
                                        None
                                    }
                                };
                                SkillRequirement {
                                    parent_skill_id,
                                    skill_id: req.skill_id,
                                    level: req.level,
                                }
                            })
                            .collect(),
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
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> CreateJob::Response<AdminAuth> {
        let Json(data) = data;
        let company = match jobs_companies::Entity::find_by_id(data.company_id)
            .one(&***db)
            .await?
        {
            Some(company) => company,
            None => {
                return CreateJob::company_not_found();
            }
        };

        let skills = self.state.services.skills.get_skills().await?;
        let (skill_requirements, not_found): (Vec<_>, Vec<_>) = data
            .skill_requirements
            .into_iter()
            .partition_map(|x| match skills.get(&x.skill_id) {
                Some(skill) => itertools::Either::Left(SkillRequirement {
                    parent_skill_id: Some(skill.parent_id.clone()),
                    ..x
                }),
                None => itertools::Either::Right(x.skill_id.clone()),
            });
        if !not_found.is_empty() {
            return CreateJob::skill_not_found(not_found);
        }

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
        .insert(&***db)
        .await?;

        jobs_skill_requirements::Entity::insert_many(skill_requirements.iter().map(|sr| {
            jobs_skill_requirements::ActiveModel {
                job_id: Set(job.id),
                skill_id: Set(sr.skill_id.clone()),
                level: Set(sr.level),
            }
        }))
        .exec(&***db)
        .await?;

        CreateJob::ok(Job::from(job, company.into(), skill_requirements))
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
    /// Skill does not exist
    SkillNotFound(404, error) => Vec<String>,
});
