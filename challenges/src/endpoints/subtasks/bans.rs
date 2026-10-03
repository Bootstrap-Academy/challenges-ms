use chrono::Utc;
use entity::{challenges_ban, sea_orm_active_enums::ChallengesBanAction};
use lib::auth::{AdminAuth, VerifiedUserAuth};
use poem::web::Data;
use poem_ext::{db::DbTxn, response};
use poem_openapi::{
    param::{Path, Query},
    payload::Json,
    OpenApi,
};
use schemas::challenges::subtasks::{Ban, CreateBanRequest, UpdateBanRequest};
use sea_orm::{ColumnTrait, Condition, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::endpoints::Tags;

pub struct Api;

#[OpenApi(tag = "Tags::Subtasks")]
impl Api {
    /// Return a list of all bans.
    ///
    /// Normal users are allowed to query their own bans by setting `user_id` to
    /// their own user id.
    #[oai(path = "/bans", method = "get")]
    pub async fn list_bans(
        &self,
        user_id: Query<Option<Uuid>>,
        creator: Query<Option<Uuid>>,
        active: Query<Option<bool>>,
        action: Query<Option<ChallengesBanAction>>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> ListBans::Response<VerifiedUserAuth> {
        if !auth.0.admin && user_id.0 != Some(auth.0.id) {
            return ListBans::permission_denied();
        }

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
                .add(challenges_ban::Column::Rescinded.eq(false))
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
        let _ = (data, db, auth);
        CreateBan::decision_required()
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
        let _ = (ban_id, data, db);
        UpdateBan::decision_required()
    }

    /// Delete a ban.
    #[oai(path = "/bans/:ban_id", method = "delete")]
    pub async fn delete_ban(
        &self,
        ban_id: Path<Uuid>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> DeleteBan::Response<AdminAuth> {
        let _ = (ban_id, db);
        DeleteBan::decision_required()
    }
}

response!(ListBans = {
    Ok(200) => Vec<Ban>,
    /// The user is not allowed to query bans of other users.
    PermissionDenied(403, error),
});

response!(CreateBan = {
    DecisionRequired(409, error),
    Created(201) => Ban,
    /// `end` cannot be before `start`
    NegativeDuration(400, error),
});

response!(UpdateBan = {
    DecisionRequired(409, error),
    Ok(200) => Ban,
    /// Ban does not exist.
    BanNotFound(404, error),
    /// `end` cannot be before `start`
    NegativeDuration(400, error),
});

response!(DeleteBan = {
    DecisionRequired(409, error),
    Ok(200),
    /// Ban does not exist.
    BanNotFound(404, error),
});
