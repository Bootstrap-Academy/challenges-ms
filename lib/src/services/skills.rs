use std::collections::HashMap;

use serde::{Deserialize, Serialize};

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
            .cached_result::<_, reqwest::Error, _, _>(
                (module_path!(), "get_skills"),
                &["skills"],
                None,
                async {
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
                },
            )
            .await??)
    }
}

#[derive(Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub parent_id: String,
}
