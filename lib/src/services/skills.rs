use std::{collections::HashMap, time::Duration};

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
            .cached_result::<_, reqwest::Error, _, _>(key!(), &["skills"], None, || async {
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
            .cached_result::<_, reqwest::Error, _, _>(key!(), &["courses"], None, || async {
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

    pub async fn get_skill_levels(&self, user_id: Uuid) -> ServiceResult<HashMap<String, u32>> {
        Ok(self
            .0
            .cache
            .cached_result(key!(user_id), &[], None, || async {
                self.0
                    .get(&format!("/skills/{user_id}"))
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await
            })
            .await??)
    }

    pub async fn get_leaderboard(
        &self,
        limit: u64,
        offset: u64,
    ) -> ServiceResult<GlobalLeaderboard> {
        Ok(self
            .0
            .json_cache
            .cached_result(
                key!(limit, offset),
                &[],
                Some(Duration::from_secs(10)),
                || async {
                    self.0
                        .get("/leaderboard")
                        .query(&[("limit", limit), ("offset", offset)])
                        .send()
                        .await?
                        .error_for_status()?
                        .json()
                        .await
                },
            )
            .await??)
    }

    pub async fn get_leaderboard_user(&self, user_id: Uuid) -> ServiceResult<Rank> {
        Ok(self
            .0
            .cache
            .cached_result(
                key!(user_id),
                &[],
                Some(Duration::from_secs(10)),
                || async {
                    self.0
                        .get(&format!("/leaderboard/{user_id}"))
                        .send()
                        .await?
                        .error_for_status()?
                        .json()
                        .await
                },
            )
            .await??)
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

#[derive(Debug, Serialize, Deserialize)]
pub struct GlobalLeaderboard {
    pub leaderboard: Vec<GlobalLeaderboardUser>,
    pub total: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GlobalLeaderboardUser {
    pub user: Uuid,
    #[serde(flatten)]
    pub rank: Rank,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Rank {
    pub xp: u64,
    pub rank: u64,
}
