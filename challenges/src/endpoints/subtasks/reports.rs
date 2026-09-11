use chrono::{DateTime, Utc};
use entity::{
    challenges_subtask_reports, challenges_subtasks, challenges_user_subtasks,
    sea_orm_active_enums::{ChallengesBanAction, ChallengesReportReason},
};
use lib::auth::{AdminAuth, VerifiedUserAuth};
use lib::SharedState;
use poem::web::Data;
use poem_ext::{db::DbTxn, response, responses::ErrorResponse};
use poem_openapi::{
    param::{Path, Query},
    payload::Json,
    types::{ParseFromJSON, ToJSON},
    OpenApi,
};
use schemas::challenges::subtasks::{CreateReportRequest, Report, ResolveReportRequest};
use sea_orm::{ActiveModelTrait, DatabaseTransaction, EntityTrait, QueryOrder, QuerySelect, Set};
use std::sync::Arc;
use uuid::Uuid;

use super::get_subtask;
use crate::{
    endpoints::Tags,
    services::subtasks::{
        get_active_ban, get_user_subtask, update_user_subtask, ActiveBan, UserSubtaskExt,
    },
};

pub struct Api {
    pub state: Arc<SharedState>,
}

#[OpenApi(tag = "Tags::Subtasks")]
impl Api {
    /// Return a list of all subtask reports.
    #[oai(path = "/subtask_reports", method = "get")]
    pub async fn list_reports(
        &self,
        /// Maximum number of reports to return
        limit: Query<Option<u64>>,
        /// Pagination offset
        offset: Query<Option<u64>>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> ListReports::Response<AdminAuth> {
        let query = challenges_subtask_reports::Entity::find()
            .find_also_related(challenges_subtasks::Entity)
            .order_by_desc(challenges_subtask_reports::Column::Timestamp)
            .limit(limit.0)
            .offset(offset.0);
        ListReports::ok(
            query
                .all(&***db)
                .await?
                .into_iter()
                .filter_map(|(report, subtask)| Some(Report::from(report, &subtask?)))
                .collect(),
        )
    }

    /// Report a subtask.
    #[oai(path = "/subtask_reports", method = "post")]
    #[allow(clippy::too_many_arguments)] // One authenticated report command and its owning transaction.
    pub async fn create_report(
        &self,
        data: Json<CreateReportRequest>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> CreateReport::Response<VerifiedUserAuth> {
        if data.0.reason == ChallengesReportReason::Dislike {
            return CreateReport::permission_denied();
        }
        let intent_id = data.0.request_id.unwrap_or_else(Uuid::new_v4);
        let request = serde_json::json!({"task_id":data.0.task_id,"subtask_id":data.0.subtask_id,"reason":format!("{:?}",data.0.reason),"comment":data.0.comment});
        crate::services::moderation::value(&db,"SELECT to_jsonb(true) AS value FROM pg_advisory_xact_lock(hashtextextended('report-intent:'||$1::uuid,0))",vec![intent_id.into()]).await?;
        let prior=crate::services::moderation::value(&db,"SELECT coalesce((SELECT jsonb_build_object('actor',actor,'matches',request_hash=encode(sha256(convert_to($2::jsonb::text,'UTF8')),'hex'),'receipt',receipt) FROM moderation_report_receipts WHERE id=$1),'null') AS value",vec![intent_id.into(),request.clone().into()]).await?;
        if !prior.is_null() {
            if prior["actor"] != serde_json::json!(auth.0.id) || prior["matches"] != true {
                return CreateReport::conflicting_request();
            }
            let mut receipt = prior["receipt"].clone();
            receipt["comment"] = serde_json::json!(data.0.comment);
            let report = Report::parse_from_json(Some(receipt))
                .map_err(|_| sea_orm::DbErr::Custom("Stored report receipt unavailable".into()))?;
            return CreateReport::created(report);
        }
        crate::services::moderation::lock_subtask(&db, data.0.subtask_id).await?;
        let Some((subtask, _)) = get_subtask(&db, data.0.task_id, data.0.subtask_id).await? else {
            return CreateReport::subtask_not_found();
        };
        if !auth.0.admin
            && (subtask.moderation_removed || (auth.0.id != subtask.creator && !subtask.enabled))
        {
            return CreateReport::subtask_not_found();
        }

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;
        if !user_subtask.can_report(&auth.0, &subtask) {
            return CreateReport::permission_denied();
        }

        match get_active_ban(&db, &auth.0, ChallengesBanAction::Report).await? {
            ActiveBan::NotBanned => {}
            ActiveBan::Temporary(end) => return CreateReport::banned(Some(end)),
            ActiveBan::Permanent => return CreateReport::banned(None),
        }

        let basis=self.state.services.auth.moderation_basis(subtask.creator).await
            .unwrap_or_else(|_|serde_json::json!({"recorded_acceptance":"unavailable","automatic_quality_basis_confirmed":false}));
        let (report, _) = create_report(
            &db,
            Some(auth.0.id),
            intent_id,
            subtask,
            user_subtask.as_ref(),
            data.0.reason,
            data.0.comment,
            basis,
        )
        .await?;

        let mut receipt = report
            .to_json()
            .ok_or_else(|| sea_orm::DbErr::Custom("Report receipt unavailable".into()))?;
        receipt.as_object_mut().unwrap().remove("comment");
        crate::services::moderation::value(&db,"WITH saved AS (INSERT INTO moderation_report_receipts(id,actor,request_hash,receipt) VALUES($1,$2,encode(sha256(convert_to($3::jsonb::text,'UTF8')),'hex'),$4) RETURNING id) SELECT to_jsonb(id) AS value FROM saved",vec![intent_id.into(),auth.0.id.into(),request.into(),receipt.into()]).await?;
        CreateReport::created(report)
    }

    /// Resolve a subtask report.
    #[oai(path = "/subtask_reports/:report_id", method = "put")]
    pub async fn resolve_report(
        &self,
        report_id: Path<Uuid>,
        data: Json<ResolveReportRequest>,
        db: Data<&DbTxn>,
        auth: AdminAuth,
    ) -> ResolveReport::Response<AdminAuth> {
        // Old requests carry neither a decision nor a recipient-safe reason.
        // Keep the URL as an explicit conflict, never delete evidence silently.
        let _ = (report_id, data, db, auth);
        ResolveReport::decision_required()
    }
}

response!(ListReports = {
    Ok(200) => Vec<Report>,
});

response!(CreateReport = {
    /// Subtask has been reported successfully.
    Created(201) => Report,
    /// This request identifier was already used with different facts.
    ConflictingRequest(409, error),
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user is not allowed to report this subtask.
    PermissionDenied(403, error),
    /// The user is currently banned from reporting subtasks.
    Banned(403, error) => Option<DateTime<Utc>>,
});

response!(ResolveReport = {
    Ok(200),
    /// Use /moderation/decisions with explicit outcome, facts and redress.
    DecisionRequired(409, error),
    /// Report not found.
    ReportNotFound(404, error),
    /// The reporter could not be banned because the report has been generated automatically.
    NoReporter(403, error),
});

#[allow(clippy::too_many_arguments)] // One authenticated report command and its owning transaction.
pub(super) async fn create_report(
    db: &DatabaseTransaction,
    user_id: Option<Uuid>,
    intent_id: Uuid,
    subtask: challenges_subtasks::Model,
    user_subtask: Option<&challenges_user_subtasks::Model>,
    reason: ChallengesReportReason,
    comment: String,
    basis: serde_json::Value,
) -> Result<(Report, challenges_subtasks::Model), ErrorResponse> {
    let now = Utc::now().naive_utc();

    if let Some(user_id) = user_id {
        update_user_subtask(
            db,
            user_subtask,
            challenges_user_subtasks::ActiveModel {
                user_id: Set(user_id),
                subtask_id: Set(subtask.id),
                rating: Set(None),
                rating_timestamp: Set(Some(now)),
                ..Default::default()
            },
        )
        .await?;
    }

    let report = challenges_subtask_reports::ActiveModel {
        id: Set(intent_id),
        subtask_id: Set(subtask.id),
        user_id: Set(user_id),
        timestamp: Set(now),
        reason: Set(reason),
        comment: Set(comment),
    }
    .insert(db)
    .await?;

    crate::services::moderation::report(
        db,
        report.id,
        user_id,
        &subtask,
        reason,
        &report.comment,
        basis,
    )
    .await?;
    let subtask = crate::services::moderation::reload_subtask(db, subtask.id).await?;

    Ok((Report::from(report, &subtask), subtask))
}
