use std::collections::HashMap;

use fnct::key;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use super::{Service, ServiceResult};

#[derive(Debug, Clone)]
pub struct SkillsService(Service);

impl SkillsService {
    pub(super) fn new(service: Service) -> Self {
        Self(service)
    }

    pub async fn get_skills(&self) -> ServiceResult<HashMap<String, Skill>> {
        Ok(self
            .0
            .cache
            .cached_result::<_, reqwest::Error, _, _>(key!(), &["skills"], None, async {
                let skills: Vec<Skill> = self
                    .0
                    .get("/skills")
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await?;
                Ok(skills
                    .into_iter()
                    .map(|skill| (skill.id.clone(), skill))
                    .collect())
            })
            .await??)
    }

    pub async fn get_courses(&self) -> ServiceResult<HashMap<String, Course>> {
        Ok(self
            .0
            .cache
            .cached_result::<_, reqwest::Error, _, _>(key!(), &["courses"], None, async {
                self.0
                    .get("/courses")
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await
            })
            .await??)
    }

    pub async fn add_skill_progress(
        &self,
        user_id: Uuid,
        skill_id: &str,
        xp: i64,
    ) -> ServiceResult<Result<(), AddSkillProgressError>> {
        let response = self
            .0
            .post(&format!("/skills/{user_id}/{skill_id}"))
            .json(&AddSkillProgressRequest { xp })
            .send()
            .await?;
        Ok(match response.status() {
            StatusCode::OK => Ok(()),
            StatusCode::NOT_FOUND => Err(AddSkillProgressError::SkillNotFound),
            code => return Err(super::ServiceError::UnexpectedStatusCode(code)),
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub parent_id: String,
    pub courses: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Course {
    pub id: String,
    pub sections: Vec<Section>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Section {
    pub id: String,
    pub lectures: Vec<Lecture>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Lecture {
    pub id: String,
}

#[derive(Debug, Serialize)]
struct AddSkillProgressRequest {
    xp: i64,
}

#[derive(Debug, Error)]
pub enum AddSkillProgressError {
    #[error("Skill not found")]
    SkillNotFound,
}
