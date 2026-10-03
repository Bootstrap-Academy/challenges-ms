//! Publication authority is never inferred from the ordinary identity cache.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{auth::AuthService, ServiceError, ServiceResult};

pub const SCOPE_VERSION: &str = "academy-verified-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicationEpoch {
    pub scope_version: String,
    pub publication_epoch: Uuid,
    pub epoch_revision: u64,
    pub policy_active: bool,
    pub publishing_enabled: bool,
}

impl PublicationEpoch {
    pub fn validate(&self) -> ServiceResult<()> {
        if self.scope_version != SCOPE_VERSION || (self.publishing_enabled && !self.policy_active) {
            return Err(ServiceError::MalformedResponse(
                "publication scope or policy",
            ));
        }
        Ok(())
    }

    pub fn require_shared(&self, enabled: bool) -> ServiceResult<()> {
        self.validate()?;
        if !enabled || !self.policy_active || !self.publishing_enabled {
            return Err(ServiceError::MalformedResponse(
                "publication is unavailable",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationParticipant {
    pub user_id: Uuid,
    pub visibility_revision: u64,
    pub display_name: String,
    pub avatar_url: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicationSnapshot {
    #[serde(flatten)]
    pub epoch: PublicationEpoch,
    pub participants: Vec<PublicationParticipant>,
}

impl PublicationSnapshot {
    pub fn validate(&self) -> ServiceResult<()> {
        self.epoch.validate()?;
        let mut ids = HashSet::new();
        if self.participants.iter().any(|person| {
            person.visibility_revision == 0
                || !person.avatar_url.is_null()
                || !ids.insert(person.user_id)
        }) {
            return Err(ServiceError::MalformedResponse("publication identity"));
        }
        Ok(())
    }

    pub fn ids(&self) -> Vec<Uuid> {
        self.participants
            .iter()
            .map(|person| person.user_id)
            .collect()
    }

    pub fn participant(&self, id: Uuid) -> Option<&PublicationParticipant> {
        self.participants.iter().find(|person| person.user_id == id)
    }
}

impl AuthService {
    /// Uncached even when the selected reader is disabled: an activated policy
    /// cannot be rolled back by changing a local application setting.
    pub async fn publication_epoch(&self) -> ServiceResult<PublicationEpoch> {
        let epoch: PublicationEpoch = self
            .0
            .get("/profile-publications/epoch")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        epoch.validate()?;
        Ok(epoch)
    }

    pub async fn publication_snapshot(&self) -> ServiceResult<PublicationSnapshot> {
        let snapshot: PublicationSnapshot = self
            .0
            .get("/profile-publications/snapshot")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        snapshot.validate()?;
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn old_or_incomplete_authority_never_grants_publication() {
        let valid = json!({"scope_version":SCOPE_VERSION,"publication_epoch":Uuid::new_v4(),
            "epoch_revision":1,"policy_active":true,"publishing_enabled":true});
        for field in [
            "scope_version",
            "publication_epoch",
            "epoch_revision",
            "policy_active",
            "publishing_enabled",
        ] {
            let mut value = valid.clone();
            value.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<PublicationEpoch>(value).is_err());
        }
        let mut epoch: PublicationEpoch = serde_json::from_value(valid).unwrap();
        assert!(epoch.require_shared(true).is_ok());
        assert!(epoch.require_shared(false).is_err());
        epoch.publishing_enabled = false;
        assert!(epoch.require_shared(true).is_err());
        epoch.scope_version = "old".into();
        assert!(epoch.validate().is_err());
    }

    #[test]
    fn incomplete_or_external_identity_never_enters_a_snapshot() {
        let value =
            json!({"user_id":Uuid::new_v4(),"visibility_revision":1,"display_name":"Shared"});
        assert!(serde_json::from_value::<PublicationParticipant>(value).is_err());
    }
}
