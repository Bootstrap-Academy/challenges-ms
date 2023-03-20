use chrono::NaiveDateTime;
use entity::{
    jobs_jobs,
    sea_orm_active_enums::{JobsJobType, JobsProfessionalLevel, JobsSalaryPer, JobsSalaryUnit},
};

use poem_openapi::Object;
use uuid::Uuid;

use super::companies::Company;

#[derive(Debug, Object)]
pub struct Job {
    /// The job's unique identifier
    pub id: Uuid,
    /// The company that posted the job
    pub company: Company,
    /// The job's title
    pub title: String,
    /// The job's description
    pub description: String,
    /// The job's location
    pub location: String,
    /// Whether the job is remote
    pub remote: bool,
    /// The job's type
    pub job_type: JobsJobType,
    /// The job's responsibilities
    pub responsibilities: Vec<String>,
    /// The job's professional level
    pub professional_level: JobsProfessionalLevel,
    /// The job's salary
    pub salary: Salary,
    /// The job's contact information
    pub contact: String,
    /// The job's last update timestamp
    pub last_update: NaiveDateTime,
    /// The job's skill requirements. Each requirement is a tuple of
    pub skill_requirements: Vec<SkillRequirement>,
}

impl Job {
    pub fn from(
        model: jobs_jobs::Model,
        company: Company,
        skill_requirements: Vec<SkillRequirement>,
    ) -> Self {
        Self {
            id: model.id,
            company,
            title: model.title,
            description: model.description,
            location: model.location,
            remote: model.remote,
            job_type: model.job_type,
            responsibilities: model.responsibilities,
            professional_level: model.professional_level,
            salary: Salary {
                min: model.salary_min,
                max: model.salary_max,
                unit: model.salary_unit,
                per: model.salary_per,
            },
            contact: model.contact,
            last_update: model.last_update,
            skill_requirements,
        }
    }
}

#[derive(Debug, Object)]
pub struct SkillRequirement {
    pub parent_skill_id: Option<String>,
    pub skill_id: String,
    pub level: i32,
}

#[derive(Debug, Object)]
pub struct Salary {
    /// Minimum salary
    pub min: i32,
    /// Maximum salary
    pub max: i32,
    /// Currency unit
    pub unit: JobsSalaryUnit,
    /// Period of time
    pub per: JobsSalaryPer,
}

#[derive(Debug, Object)]
pub struct CreateJobRequest {
    /// The company that posted the job
    pub company_id: Uuid,
    /// The job's title
    pub title: String,
    /// The job's description
    pub description: String,
    /// The job's location
    pub location: String,
    /// Whether the job is remote
    pub remote: bool,
    /// The job's type
    pub job_type: JobsJobType,
    /// The job's responsibilities
    pub responsibilities: Vec<String>,
    /// The job's professional level
    pub professional_level: JobsProfessionalLevel,
    /// The job's salary
    pub salary: Salary,
    /// The job's contact information
    pub contact: String,
    /// The job's skill requirements. Each requirement is a tuple of
    pub skill_requirements: Vec<SkillRequirement>,
}
