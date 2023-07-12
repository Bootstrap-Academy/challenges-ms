use chrono::Utc;
use entity::{challenges_ban, sea_orm_active_enums::ChallengesBanAction};
use lib::auth::AdminAuth;
use poem::web::Data;
use poem_ext::{db::DbTxn, patch_value::PatchValue, response};
use poem_openapi::{
    param::{Path, Query},
    payload::Json,
    OpenApi,
};
use schemas::challenges::subtasks::{Ban, CreateBanRequest, UpdateBanRequest};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseTransaction, DbErr, EntityTrait, ModelTrait,
    QueryFilter, Set, Unchanged,
};
use uuid::Uuid;

use crate::endpoints::Tags;

pub struct Api;

#[OpenApi(tag = "Tags::Subtasks")]
impl Api {
    /// Return a list of all bans.
    #[oai(path = "/bans", method = "get")]
    pub async fn list_bans(
        &self,
        user_id: Query<Option<String>>,
        creator: Query<Option<String>>,
        active: Query<Option<bool>>,
        action: Query<Option<ChallengesBanAction>>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> ListBans::Response<AdminAuth> {
        let mut query = challenges_ban::Entity::find();
        if let Some(user_id) = user_id.0 {
            query = query.filter(challenges_ban::Column::UserId.eq(user_id));
        }
        if let Some(creator) = creator.0 {
            query = query.filter(challenges_ban::Column::Creator.eq(creator));
        }
        if let Some(active) = active.0 {
            let now = Utc::now();
            let mut cond = Condition::all()
                .add(challenges_ban::Column::Start.lte(now))
                .add(
                    Condition::any()
                        .add(challenges_ban::Column::End.is_null())
                        .add(challenges_ban::Column::End.gt(now)),
                );
            if !active {
                cond = cond.not();
            }
            query = query.filter(cond);
        }
        if let Some(action) = action.0 {
            query = query.filter(challenges_ban::Column::Action.eq(action));
        }
        ListBans::ok(
            query
                .all(&***db)
                .await?
                .into_iter()
                .map(Into::into)
                .collect(),
        )
    }

    /// Create a new ban.
    #[oai(path = "/bans", method = "post")]
    pub async fn create_ban(
        &self,
        data: Json<CreateBanRequest>,
        db: Data<&DbTxn>,
        auth: AdminAuth,
    ) -> CreateBan::Response<AdminAuth> {
        let start = data.0.start.unwrap_or(Utc::now());
        if data.0.end.is_some_and(|ts| ts <= start) {
            return CreateBan::negative_duration();
        }

        CreateBan::created(
            challenges_ban::ActiveModel {
                id: Set(Uuid::new_v4()),
                user_id: Set(data.0.user_id),
                creator: Set(auth.0.id),
                start: Set(start.naive_utc()),
                end: Set(data.0.end.map(|ts| ts.naive_utc())),
                action: Set(data.0.action),
                reason: Set(data.0.reason),
            }
            .insert(&***db)
            .await?
            .into(),
        )
    }

    /// Update a ban.
    #[oai(path = "/bans/:ban_id", method = "patch")]
    pub async fn update_ban(
        &self,
        ban_id: Path<Uuid>,
        data: Json<UpdateBanRequest>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> UpdateBan::Response<AdminAuth> {
        let Some(ban) = get_ban(&db, ban_id.0).await? else {
            return UpdateBan::ban_not_found();
        };

        let mut data = data.0;
        if data.permanent {
            data.end = PatchValue::Set(None);
        }

        let start = *data.start.get_new(&ban.start.and_utc());
        let end = *data.end.get_new(&ban.end.map(|ts| ts.and_utc()));
        if end.is_some_and(|ts| ts <= start) {
            return UpdateBan::negative_duration();
        }

        UpdateBan::ok(
            challenges_ban::ActiveModel {
                id: Unchanged(ban.id),
                user_id: Unchanged(ban.user_id),
                creator: Unchanged(ban.creator),
                start: data.start.map(|ts| ts.naive_utc()).update(ban.start),
                end: data.end.map(|x| x.map(|ts| ts.naive_utc())).update(ban.end),
                action: data.action.update(ban.action),
                reason: data.reason.update(ban.reason),
            }
            .update(&***db)
            .await?
            .into(),
        )
    }

    /// Delete a ban.
    #[oai(path = "/bans/:ban_id", method = "delete")]
    pub async fn delete_ban(
        &self,
        ban_id: Path<Uuid>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> DeleteBan::Response<AdminAuth> {
        let Some(ban) = get_ban(&db, ban_id.0).await? else {
            return DeleteBan::ban_not_found();
        };

        ban.delete(&***db).await?;
        DeleteBan::ok()
    }
}

response!(ListBans = {
    Ok(200) => Vec<Ban>,
});

response!(CreateBan = {
    Created(201) => Ban,
    /// `end` cannot be before `start`
    NegativeDuration(400, error),
});

response!(UpdateBan = {
    Ok(200) => Ban,
    /// Ban does not exist.
    BanNotFound(404, error),
    /// `end` cannot be before `start`
    NegativeDuration(400, error),
});

response!(DeleteBan = {
    Ok(200),
    /// Ban does not exist.
    BanNotFound(404, error),
});

async fn get_ban(
    db: &DatabaseTransaction,
    ban_id: Uuid,
) -> Result<Option<challenges_ban::Model>, DbErr> {
    challenges_ban::Entity::find_by_id(ban_id).one(db).await
}
