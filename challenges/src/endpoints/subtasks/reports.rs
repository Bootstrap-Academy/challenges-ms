use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use entity::{
    challenges_ban, challenges_subtask_reports, challenges_subtasks, challenges_user_subtasks,
    sea_orm_active_enums::{ChallengesBanAction, ChallengesReportReason},
};
use lib::{
    auth::{AdminAuth, VerifiedUserAuth},
    config::Config,
};
use poem::web::Data;
use poem_ext::{db::DbTxn, response, responses::ErrorResponse};
use poem_openapi::{
    param::{Path, Query},
    payload::Json,
    OpenApi,
};
use schemas::challenges::subtasks::{
    CreateReportRequest, Report, ResolveReportAction, ResolveReportRequest,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, ModelTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Set,
};
use uuid::Uuid;

use super::get_subtask;
use crate::{
    endpoints::Tags,
    services::subtasks::{
        get_active_ban, get_user_subtask, update_user_subtask, ActiveBan, UserSubtaskExt,
    },
};

pub struct Api {
    pub config: Arc<Config>,
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
    pub async fn create_report(
        &self,
        data: Json<CreateReportRequest>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> CreateReport::Response<VerifiedUserAuth> {
        let Some((subtask, _)) = get_subtask(&db, data.0.task_id, data.0.subtask_id).await? else {
            return CreateReport::subtask_not_found();
        };
        if !auth.0.admin && auth.0.id != subtask.creator && !subtask.enabled {
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

        let (report, _) = create_report(
            &db,
            Some(auth.0.id),
            subtask,
            user_subtask.as_ref(),
            data.0.reason,
            data.0.comment,
        )
        .await?;

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
        let Some((report, Some(subtask))) =
            challenges_subtask_reports::Entity::find_by_id(report_id.0)
                .find_also_related(challenges_subtasks::Entity)
                .one(&***db)
                .await?
        else {
            return ResolveReport::report_not_found();
        };

        let subtask_deleted = match data.0.action {
            ResolveReportAction::Revise => false,
            ResolveReportAction::BlockReporter => {
                let Some(reporter) = report.user_id else {
                    return ResolveReport::no_reporter();
                };
                ban_user(
                    &db,
                    reporter,
                    ChallengesBanAction::Report,
                    &self.config.challenges.quizzes.ban_days,
                    auth.0.id,
                    format!("Bad report ({}): {}", report.id, report.comment),
                )
                .await?;
                challenges_subtasks::ActiveModel {
                    enabled: Set(true),
                    ..subtask.into()
                }
                .update(&***db)
                .await?;
                false
            }
            ResolveReportAction::BlockCreator => {
                ban_user(
                    &db,
                    subtask.creator,
                    ChallengesBanAction::Create,
                    &self.config.challenges.quizzes.ban_days,
                    auth.0.id,
                    format!("Bad subtask: {}", report.comment),
                )
                .await?;
                subtask.delete(&***db).await?;
                true
            }
        };

        if !subtask_deleted {
            report.delete(&***db).await?;
        }

        ResolveReport::ok()
    }
}

response!(ListReports = {
    Ok(200) => Vec<Report>,
});

response!(CreateReport = {
    /// Subtask has been reported successfully.
    Created(201) => Report,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user is not allowed to report this subtask.
    PermissionDenied(403, error),
    /// The user is currently banned from reporting subtasks.
    Banned(403, error) => Option<DateTime<Utc>>,
});

response!(ResolveReport = {
    Ok(200),
    /// Report not found.
    ReportNotFound(404, error),
    /// The reporter could not be banned because the report has been generated automatically.
    NoReporter(403, error),
});

pub(super) async fn create_report(
    db: &DatabaseTransaction,
    user_id: Option<Uuid>,
    subtask: challenges_subtasks::Model,
    user_subtask: Option<&challenges_user_subtasks::Model>,
    reason: ChallengesReportReason,
    comment: String,
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
        id: Set(Uuid::new_v4()),
        subtask_id: Set(subtask.id),
        user_id: Set(user_id),
        timestamp: Set(now),
        reason: Set(reason),
        comment: Set(comment),
    }
    .insert(db)
    .await?;

    let subtask = challenges_subtasks::ActiveModel {
        enabled: Set(false),
        ..subtask.into()
    }
    .update(db)
    .await?;

    Ok((Report::from(report, &subtask), subtask))
}

async fn ban_user(
    db: &DatabaseTransaction,
    user_id: Uuid,
    action: ChallengesBanAction,
    ban_days: &[u32],
    creator: Uuid,
    reason: String,
) -> Result<challenges_ban::Model, ErrorResponse> {
    let now = Utc::now().naive_utc();

    let bans = challenges_ban::Entity::find()
        .filter(challenges_ban::Column::UserId.eq(user_id))
        .filter(challenges_ban::Column::Action.eq(action))
        .count(db)
        .await?;

    let duration = ban_days
        .get(bans as usize)
        .map(|&days| Duration::days(days as _));

    Ok(challenges_ban::ActiveModel {
        id: Set(Uuid::new_v4()),
        user_id: Set(user_id),
        start: Set(now),
        end: Set(duration.map(|duration| now + duration)),
        action: Set(action),
        creator: Set(creator),
        reason: Set(reason),
    }
    .insert(db)
    .await?)
}
