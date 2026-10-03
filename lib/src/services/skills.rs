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
    /// Deliberately bypass the legacy rank cache and require the new capability.
    pub async fn published_leaderboard(
        &self,
        limit: u64,
        offset: u64,
        epoch: &super::publications::PublicationEpoch,
    ) -> ServiceResult<PublishedGlobalLeaderboard> {
        Ok(self
            .0
            .get("/published-leaderboard")
            .query(&[
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
                ("publication_epoch", epoch.publication_epoch.to_string()),
                ("scope_version", epoch.scope_version.clone()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn published_rank(
        &self,
        user_id: Uuid,
        epoch: &super::publications::PublicationEpoch,
    ) -> ServiceResult<PublishedGlobalRank> {
        Ok(self
            .0
            .get(&format!("/published-leaderboard/{user_id}"))
            .query(&[
                ("publication_epoch", epoch.publication_epoch.to_string()),
                ("scope_version", epoch.scope_version.clone()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    /// Bounded, ordered read decisions preserve concrete lesson scope.
    pub async fn learning_access_reads(
        &self,
        user: Uuid,
        requests: &[LearningAccessRequest],
    ) -> ServiceResult<Vec<bool>> {
        if requests.is_empty()
            || requests.len() > 250
            || requests.iter().any(|request| request.request_id.is_some())
        {
            return Err(super::ServiceError::MalformedResponse(
                "Invalid learning read batch",
            ));
        }
        let response = self
            .0
            .post(&format!("/learning-access/{user}/check-batch"))
            .json(&serde_json::json!({"requests": requests}))
            .timeout(Duration::from_secs(10))
            .send()
            .await?;
        if response.status() != StatusCode::OK {
            return Err(super::ServiceError::UnexpectedStatusCode(response.status()));
        }
        #[derive(Deserialize)]
        struct Decisions {
            readable: Vec<bool>,
        }
        let decisions: Decisions = response.json().await?;
        if decisions.readable.len() != requests.len() {
            return Err(super::ServiceError::MalformedResponse(
                "Incomplete learning read batch",
            ));
        }
        Ok(decisions.readable)
    }

    /// Checks and starts share one authority. Checks never consume a lesson.
    pub async fn learning_access(
        &self,
        user: Uuid,
        request: &LearningAccessRequest,
    ) -> ServiceResult<Result<LearningAccessAllowed, LearningAccessDenied>> {
        let action = if request.request_id.is_some() {
            "start"
        } else {
            "check"
        };
        let response = self
            .0
            .post(&format!("/learning-access/{user}/{action}"))
            .json(request)
            .timeout(Duration::from_secs(10))
            .send()
            .await?;
        let status = response.status();
        if status == StatusCode::OK {
            let result: serde_json::Value = response.json().await?;
            if result["allowed"] != true {
                return Err(super::ServiceError::MalformedResponse(
                    "Invalid learning admission",
                ));
            }
            let allowed = serde_json::from_value(result).map_err(|_| {
                super::ServiceError::MalformedResponse("Invalid learning heart policy")
            })?;
            return Ok(Ok(allowed));
        }
        if matches!(
            status,
            StatusCode::FORBIDDEN
                | StatusCode::NOT_FOUND
                | StatusCode::CONFLICT
                | StatusCode::TOO_MANY_REQUESTS
        ) {
            let body: serde_json::Value = response.json().await?;
            if !body.is_object()
                || (status == StatusCode::TOO_MANY_REQUESTS
                    && (body["code"] != "daily_limit_reached" || !body["daily"].is_object()))
            {
                return Err(super::ServiceError::MalformedResponse(
                    "Invalid learning refusal",
                ));
            }
            return Ok(Err(LearningAccessDenied {
                status: status.as_u16(),
                body,
            }));
        }
        Err(super::ServiceError::UnexpectedStatusCode(status))
    }

    pub async fn apply_benefit(
        &self,
        operation: Uuid,
        user: Uuid,
        request: &serde_json::Value,
    ) -> ServiceResult<serde_json::Value> {
        let Some(skill) = request["skill_id"].as_str() else {
            return Ok(serde_json::json!({"state":"review","reason":"Missing original skill"}));
        };
        // Push the skill as one path segment; a configured skill is not a URL.
        let path = format!("/xp-operations/{operation}/{user}/");
        let mut url = self
            .0
            .base_url
            .join(&format!("_internal/{}", path.trim_start_matches('/')))
            .expect("fixed benefit URL");
        url.path_segments_mut()
            .expect("service URL supports paths")
            .pop_if_empty()
            .push(skill);
        let body = serde_json::json!({"xp":request["xp"],"earning_id":request["earning_id"]});
        let response = self
            .0
            .request_url(reqwest::Method::POST, url)
            .json(&body)
            .timeout(Duration::from_secs(10))
            .send()
            .await?;
        if response.status() == StatusCode::CONFLICT {
            return Ok(
                serde_json::json!({"state":"review","reason":"Exact benefit payload conflict"}),
            );
        }
        if response.status() != StatusCode::OK {
            return Err(super::ServiceError::UnexpectedStatusCode(response.status()));
        }
        let result: serde_json::Value = response.json().await?;
        let expected = serde_json::json!({"user_id":user,"skill_id":skill,"xp":request["xp"],"earning_id":request["earning_id"]});
        if result["operation_id"] != serde_json::json!(operation)
            || result["request"] != expected
            || !((result["state"] == "applied" && result["applied"] == true)
                || (result["state"] == "recipient_erased" && result["applied"] == false))
        {
            return Ok(
                serde_json::json!({"state":"uncertain","reason":"Unrecognized exact benefit receipt"}),
            );
        }
        Ok(result)
    }

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
            .cached_result(key!(user_id), &[&format!("{user_id}")], None, || async {
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
                &[&format!("{user_id}")],
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

#[derive(Debug, Serialize)]
pub struct LectureBinding {
    pub course_id: String,
    pub section_id: Option<String>,
    pub lecture_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LearningAccessRequest {
    pub task_id: Option<Uuid>,
    pub subtask_id: Option<Uuid>,
    pub lecture_bindings: Vec<LectureBinding>,
    pub user_admin: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<Uuid>,
}

/// Absent on older Skills generations. Admission alone is never a billing exemption.
#[derive(Debug, Default, Deserialize)]
pub struct LearningAccessAllowed {
    pub heart_policy: Option<LearningHeartPolicy>,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearningHeartPolicy {
    Legacy,
    Daily,
}

#[derive(Debug)]
pub struct LearningAccessDenied {
    pub status: u16,
    pub body: serde_json::Value,
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedGlobalLeaderboard {
    pub leaderboard: Vec<GlobalLeaderboardUser>,
    pub total: u64,
    pub scope_version: String,
    pub publication_epoch: Uuid,
    pub epoch_revision: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedGlobalRank {
    pub xp: u64,
    pub rank: Option<u64>,
    pub public_rank: Option<u64>,
    pub scope_version: String,
    pub publication_epoch: Uuid,
    pub epoch_revision: u64,
}
