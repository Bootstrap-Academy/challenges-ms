//! Every exercise entry point uses Skills' course/lesson authority. Parent
//! bindings come from this database, never from caller-supplied course IDs.
use std::collections::{HashMap, HashSet};

use entity::{challenges_course_tasks, challenges_subtasks};
use lib::{
    auth::User,
    services::{
        skills::{
            LearningAccessAllowed, LearningAccessDenied, LearningAccessRequest,
            LearningHeartPolicy, LectureBinding,
        },
        ServiceError, Services,
    },
};
use poem::{http::StatusCode, Response};
use poem_ext::responses::{InnerResponse, MetaResponsesExt};
use poem_openapi::ApiResponse;
use sea_orm::{ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter};
use uuid::Uuid;

pub struct Denied(pub LearningAccessDenied);

impl Denied {
    pub fn unavailable() -> Self {
        Self(LearningAccessDenied {
            status: 503,
            body: serde_json::json!({"code":"learning_access_unavailable","detail":"Dein Lernzugang ist gerade nicht erreichbar. Versuch es gleich noch einmal."}),
        })
    }

    pub fn response<T: ApiResponse, A: MetaResponsesExt>(
        self,
    ) -> poem_ext::responses::Response<T, A> {
        let response = Response::builder()
            .status(StatusCode::from_u16(self.0.status).expect("validated service status"))
            .content_type("application/json")
            .body(self.0.body.to_string());
        Ok(InnerResponse::from_parse_request_error(
            poem::Error::from_response(response),
        ))
    }
}

async fn request(
    services: &Services,
    user: &User,
    task_id: Option<Uuid>,
    subtask_id: Option<Uuid>,
    binding: Option<&challenges_course_tasks::Model>,
    request_id: Option<Uuid>,
) -> anyhow::Result<Result<LearningAccessAllowed, Denied>> {
    if user.admin {
        return Ok(Ok(LearningAccessAllowed::default()));
    }
    let request = admission_request(user, task_id, subtask_id, binding, request_id);
    match services.skills.learning_access(user.id, &request).await {
        Ok(result) => Ok(result.map_err(Denied)),
        Err(ServiceError::UnexpectedStatusCode(status)) if status.is_server_error() => {
            Ok(Err(Denied::unavailable()))
        }
        Err(ServiceError::ReqwestError(error)) if error.is_connect() || error.is_timeout() => {
            Ok(Err(Denied::unavailable()))
        }
        Err(error) => Err(error.into()),
    }
}

fn admission_request(
    user: &User,
    task_id: Option<Uuid>,
    subtask_id: Option<Uuid>,
    binding: Option<&challenges_course_tasks::Model>,
    request_id: Option<Uuid>,
) -> LearningAccessRequest {
    LearningAccessRequest {
        task_id,
        subtask_id,
        lecture_bindings: binding
            .map(|b| LectureBinding {
                course_id: b.course_id.clone(),
                section_id: b.section_id.clone(),
                lecture_id: b.lecture_id.clone(),
            })
            .into_iter()
            .collect(),
        user_admin: user.admin,
        request_id,
    }
}

pub async fn start(
    db: &DatabaseTransaction,
    services: &Services,
    user: &User,
    subtask: &challenges_subtasks::Model,
    request_id: Uuid,
) -> anyhow::Result<Option<Denied>> {
    if subtask.retired || user.admin || user.id == subtask.creator {
        return Ok(None);
    }
    let binding = challenges_course_tasks::Entity::find_by_id(subtask.task_id)
        .one(db)
        .await?;
    request(
        services,
        user,
        Some(subtask.task_id),
        Some(subtask.id),
        binding.as_ref(),
        Some(request_id),
    )
    .await
    .map(|result| result.err())
}

/// Only the authenticated Skills authority can identify the concrete lesson's
/// durable Daily policy during a Shop outage. A generic allowed/null response
/// (including old Skills versions) is insufficient to waive legacy hearts.
pub async fn daily_heart_exemption(
    db: &DatabaseTransaction,
    services: &Services,
    user: &User,
    subtask: &challenges_subtasks::Model,
) -> anyhow::Result<bool> {
    let binding = challenges_course_tasks::Entity::find_by_id(subtask.task_id)
        .one(db)
        .await?;
    Ok(matches!(
        request(
            services,
            user,
            Some(subtask.task_id),
            Some(subtask.id),
            binding.as_ref(),
            None
        )
        .await?,
        Ok(LearningAccessAllowed {
            heart_policy: Some(LearningHeartPolicy::Daily)
        })
    ))
}

pub async fn can_read_subtask(
    db: &DatabaseTransaction,
    services: &Services,
    user: &User,
    subtask: &challenges_subtasks::Model,
) -> anyhow::Result<bool> {
    if subtask.retired || user.admin || user.id == subtask.creator {
        return Ok(true);
    }
    let binding = challenges_course_tasks::Entity::find_by_id(subtask.task_id)
        .one(db)
        .await?;
    readable(
        request(
            services,
            user,
            Some(subtask.task_id),
            Some(subtask.id),
            binding.as_ref(),
            None,
        )
        .await?
        .err(),
    )
}

/// Disabled lists have no new service dependency. Active lists retain each
/// concrete subtask's decision: a Task-wide check can widen outage admission.
pub async fn retain_readable_subtasks(
    db: &DatabaseTransaction,
    services: &Services,
    user: &User,
    subtasks: Vec<challenges_subtasks::Model>,
    enabled: bool,
) -> anyhow::Result<Vec<challenges_subtasks::Model>> {
    if !enabled || user.admin {
        return Ok(subtasks);
    }
    let pending: Vec<_> = subtasks
        .iter()
        .filter(|subtask| !subtask.retired && subtask.creator != user.id)
        .collect();
    if pending.is_empty() {
        return Ok(subtasks);
    }
    let task_ids: HashSet<_> = pending.iter().map(|subtask| subtask.task_id).collect();
    let bindings: HashMap<_, _> = challenges_course_tasks::Entity::find()
        .filter(challenges_course_tasks::Column::TaskId.is_in(task_ids))
        .all(db)
        .await?
        .into_iter()
        .map(|binding| (binding.task_id, binding))
        .collect();
    let mut denied = HashSet::new();
    for batch in pending.chunks(250) {
        let requests: Vec<_> = batch
            .iter()
            .map(|subtask| {
                admission_request(
                    user,
                    Some(subtask.task_id),
                    Some(subtask.id),
                    bindings.get(&subtask.task_id),
                    None,
                )
            })
            .collect();
        let decisions = services
            .skills
            .learning_access_reads(user.id, &requests)
            .await?;
        for (subtask, allowed) in batch.iter().zip(decisions) {
            if !allowed {
                denied.insert(subtask.id);
            }
        }
    }
    Ok(subtasks
        .into_iter()
        .filter(|s| !denied.contains(&s.id))
        .collect())
}

pub async fn can_read_course_task(
    services: &Services,
    user: &User,
    binding: &challenges_course_tasks::Model,
) -> anyhow::Result<bool> {
    readable(
        request(
            services,
            user,
            Some(binding.task_id),
            None,
            Some(binding),
            None,
        )
        .await?
        .err(),
    )
}

fn readable(denial: Option<Denied>) -> anyhow::Result<bool> {
    match denial {
        None => Ok(true),
        Some(Denied(result)) if matches!(result.status, 403 | 404) => Ok(false),
        Some(_) => anyhow::bail!("Unexpected learning read refusal"),
    }
}
