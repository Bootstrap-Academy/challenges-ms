use crate::services::moderation as cases;
use entity::challenges_subtasks;
use lib::auth::{AdminAuth, InternalAuth, UserAuth};
use poem::web::Data;
use poem_ext::{db::DbTxn, response};
use poem_openapi::{
    param::{Path, Query},
    payload::Json,
    OpenApi,
};
use sea_orm::EntityTrait;
use serde_json::{json, Value};
use uuid::Uuid;

pub struct Api {
    pub state: std::sync::Arc<lib::SharedState>,
}

#[OpenApi(tag = "super::Tags::Subtasks")]
impl Api {
    #[oai(path = "/moderation/inbox", method = "get")]
    async fn inbox(&self, db: Data<&DbTxn>, auth: UserAuth) -> Read::Response<UserAuth> {
        Read::ok(cases::inbox(&db, auth.0.id).await?)
    }
    #[oai(path = "/moderation/complaints", method = "post")]
    async fn complain(
        &self,
        db: Data<&DbTxn>,
        auth: UserAuth,
        body: Json<Value>,
    ) -> Write::Response<UserAuth> {
        match cases::value(
            &db,
            "SELECT to_jsonb(moderation_complain($1,$2)) AS value",
            vec![auth.0.id.into(), body.0.into()],
        )
        .await
        {
            Ok(id) => Write::ok(json!({"id":id,"status":"pending_human_review"})),
            Err(_) => Write::conflict(),
        }
    }
    #[oai(path = "/moderation/cases", method = "get")]
    async fn queue(
        &self,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
        limit: Query<Option<i32>>,
        offset: Query<Option<i32>>,
    ) -> Read::Response<AdminAuth> {
        Read::ok(
            cases::value(
                &db,
                "SELECT moderation_queue($1,$2) AS value",
                vec![limit.0.unwrap_or(50).into(), offset.0.unwrap_or(0).into()],
            )
            .await?,
        )
    }
    #[oai(path = "/moderation/cases/:id", method = "get")]
    async fn case(
        &self,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
        id: Path<Uuid>,
    ) -> Read::Response<AdminAuth> {
        let mut record=cases::value(&db,"SELECT coalesce((SELECT to_jsonb(c)||jsonb_build_object('decisions',(SELECT coalesce(jsonb_agg(to_jsonb(d) ORDER BY d.created_at),'[]') FROM moderation_decisions d WHERE d.case_id=c.id),'complaints',(SELECT coalesce(jsonb_agg(to_jsonb(a)),'[]') FROM moderation_complaints a WHERE a.case_id=c.id),'escalations',(SELECT coalesce(jsonb_agg(to_jsonb(e)),'[]') FROM moderation_escalations e WHERE e.case_id=c.id),'retention_reviews',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY r.recorded_at),'[]') FROM moderation_retention_reviews r WHERE r.case_id=c.id),'effective',moderation_effect(c.target_kind,c.target_id)) FROM moderation_cases c WHERE c.id=$1),'null') AS value",vec![id.0.into()]).await?;
        if !record.is_null() {
            record["review_target"] = cases::review_target(&db, &record).await?;
        }
        Read::ok(record)
    }
    #[oai(path = "/moderation/cases", method = "post")]
    async fn open(
        &self,
        db: Data<&DbTxn>,
        auth: AdminAuth,
        body: Json<Value>,
    ) -> Write::Response<AdminAuth> {
        let r = body.0;
        let (Some(id), Some(target), Some(kind), Some(source), Some(evidence)) = (
            uuid(&r, "id"),
            uuid(&r, "target_id"),
            r["target_kind"].as_str(),
            r["source"].as_str(),
            r.get("private_evidence"),
        ) else {
            return Write::conflict();
        };
        if !["own_review", "email_notice", "authority_order"].contains(&source)
            || !evidence.is_object()
        {
            return Write::conflict();
        }
        if kind == "subtask" {
            cases::lock_subtask(&db, target).await?;
            let Some(subtask) = challenges_subtasks::Entity::find_by_id(target)
                .one(&***db)
                .await?
            else {
                return Write::conflict();
            };
            // Subject and parent are always resolved from the exact stored ID.
            if cases::open_subtask(
                &db,
                id,
                Some(auth.0.id),
                &subtask,
                source,
                uuid(&r, "notifier"),
                evidence.clone(),
            )
            .await
            .is_err()
            {
                return Write::conflict();
            }
        } else if ["create", "report"].contains(&kind) {
            let Some(_) = self
                .state
                .services
                .auth
                .get_user_by_id_uncached(target)
                .await
                .map_err(poem_ext::responses::ErrorResponse::from)?
            else {
                return Write::conflict();
            };
            let basis = self
                .state
                .services
                .auth
                .moderation_basis(target)
                .await
                .map_err(poem_ext::responses::ErrorResponse::from)?;
            let mut evidence = evidence.clone();
            evidence["author_contact"] = basis["contact"].clone();
            evidence["rule_evidence"] = basis;

            if cases::value(
                &db,
                "SELECT to_jsonb(moderation_open($1,$2,$3,$4,$4,$5,$6,$7)) AS value",
                vec![
                    id.into(),
                    auth.0.id.into(),
                    kind.into(),
                    target.into(),
                    source.into(),
                    uuid(&r, "notifier").into(),
                    evidence.into(),
                ],
            )
            .await
            .is_err()
            {
                return Write::conflict();
            }
        } else {
            return Write::conflict();
        }
        Write::ok(json!({"id":id,"revision":0}))
    }
    #[oai(path = "/moderation/decisions", method = "post")]
    async fn decide(
        &self,
        db: Data<&DbTxn>,
        auth: AdminAuth,
        body: Json<Value>,
    ) -> Write::Response<AdminAuth> {
        match cases::decide(&db, auth.0.id, body.0).await {
            Ok(v) => Write::ok(v),
            Err(_) => Write::conflict(),
        }
    }
    #[oai(path = "/moderation/escalations", method = "post")]
    async fn escalate(
        &self,
        db: Data<&DbTxn>,
        auth: AdminAuth,
        body: Json<Value>,
    ) -> Write::Response<AdminAuth> {
        match cases::value(
            &db,
            "SELECT to_jsonb(moderation_escalate($1,$2)) AS value",
            vec![auth.0.id.into(), body.0.into()],
        )
        .await
        {
            Ok(v) => Write::ok(json!({"id":v})),
            Err(_) => Write::conflict(),
        }
    }
    #[oai(path = "/moderation/retention", method = "post")]
    async fn retention(
        &self,
        db: Data<&DbTxn>,
        auth: AdminAuth,
        body: Json<Value>,
    ) -> Write::Response<AdminAuth> {
        match cases::value(
            &db,
            "SELECT to_jsonb(moderation_retention($1,$2)) AS value",
            vec![auth.0.id.into(), body.0.into()],
        )
        .await
        {
            Ok(v) => Write::ok(v),
            Err(_) => Write::conflict(),
        }
    }
    #[oai(path = "/_internal/moderation/maintenance", method = "post")]
    async fn maintenance(
        &self,
        db: Data<&DbTxn>,
        _auth: InternalAuth,
    ) -> Read::Response<InternalAuth> {
        Read::ok(
            cases::value(
                &db,
                "SELECT to_jsonb(moderation_maintenance()) AS value",
                vec![],
            )
            .await?,
        )
    }
    #[oai(path = "/_internal/moderation/minimizations", method = "get")]
    async fn minimizations(
        &self,
        db: Data<&DbTxn>,
        _auth: InternalAuth,
    ) -> Read::Response<InternalAuth> {
        Read::ok(cases::value(&db,"SELECT coalesce(jsonb_agg(to_jsonb(d)),'[]') AS value FROM (SELECT id,case_id,field FROM moderation_private_minimizations WHERE relayed_at IS NULL ORDER BY created_at LIMIT 25) d",vec![]).await?)
    }
    #[oai(path = "/_internal/moderation/minimizations/ack", method = "post")]
    async fn minimization_ack(
        &self,
        db: Data<&DbTxn>,
        _auth: InternalAuth,
        body: Json<Value>,
    ) -> Write::Response<InternalAuth> {
        let Some(id) = uuid(&body.0, "id") else {
            return Write::conflict();
        };
        Write::ok(cases::value(&db,"WITH updated AS (UPDATE moderation_private_minimizations SET relayed_at=coalesce(relayed_at,clock_timestamp()) WHERE id=$1 RETURNING id) SELECT to_jsonb(EXISTS(SELECT 1 FROM updated)) AS value",vec![id.into()]).await?)
    }
    #[oai(path = "/_internal/moderation/disposals", method = "get")]
    async fn disposals(
        &self,
        db: Data<&DbTxn>,
        _auth: InternalAuth,
    ) -> Read::Response<InternalAuth> {
        Read::ok(cases::value(&db,"SELECT coalesce(jsonb_agg(to_jsonb(d)),'[]') AS value FROM (SELECT case_id,disposed_at FROM moderation_disposals WHERE relayed_at IS NULL ORDER BY disposed_at LIMIT 25) d",vec![]).await?)
    }
    #[oai(path = "/_internal/moderation/disposals/ack", method = "post")]
    async fn disposal_ack(
        &self,
        db: Data<&DbTxn>,
        _auth: InternalAuth,
        body: Json<Value>,
    ) -> Write::Response<InternalAuth> {
        let Some(id) = uuid(&body.0, "case_id") else {
            return Write::conflict();
        };
        Write::ok(cases::value(&db,"WITH updated AS (UPDATE moderation_disposals SET relayed_at=coalesce(relayed_at,clock_timestamp()) WHERE case_id=$1 RETURNING case_id) SELECT to_jsonb(EXISTS(SELECT 1 FROM updated)) AS value",vec![id.into()]).await?)
    }
    // Backend validates a separate opaque capability before using these narrow
    // endpoints. They do not accept a caller-chosen recipient via ordinary auth.
    #[oai(path = "/_internal/moderation/recipients/:user/inbox", method = "get")]
    async fn retained_inbox(
        &self,
        db: Data<&DbTxn>,
        _auth: InternalAuth,
        user: Path<Uuid>,
    ) -> Read::Response<InternalAuth> {
        Read::ok(cases::inbox(&db, user.0).await?)
    }
    #[oai(
        path = "/_internal/moderation/recipients/:user/complaints",
        method = "post"
    )]
    async fn retained_complain(
        &self,
        db: Data<&DbTxn>,
        _auth: InternalAuth,
        user: Path<Uuid>,
        body: Json<Value>,
    ) -> Write::Response<InternalAuth> {
        match cases::value(
            &db,
            "SELECT to_jsonb(moderation_complain($1,$2)) AS value",
            vec![user.0.into(), body.0.into()],
        )
        .await
        {
            Ok(v) => Write::ok(json!({"id":v,"status":"pending_human_review"})),
            Err(_) => Write::conflict(),
        }
    }
    #[oai(path = "/_internal/moderation/delivery/claim", method = "post")]
    async fn claim(&self, db: Data<&DbTxn>, _auth: InternalAuth) -> Read::Response<InternalAuth> {
        Read::ok(cases::value(&db, "SELECT moderation_claim(25) AS value", vec![]).await?)
    }
    #[oai(
        path = "/_internal/moderation/recipients/:user/opened",
        method = "post"
    )]
    async fn retained_opened(
        &self,
        db: Data<&DbTxn>,
        _auth: InternalAuth,
        user: Path<Uuid>,
        body: Json<Value>,
    ) -> Write::Response<InternalAuth> {
        let Some(id) = uuid(&body.0, "id") else {
            return Write::conflict();
        };
        Write::ok(
            cases::value(
                &db,
                "SELECT to_jsonb(moderation_opened($1,$2)) AS value",
                vec![user.0.into(), id.into()],
            )
            .await?,
        )
    }
    #[oai(path = "/_internal/moderation/delivery/ack", method = "post")]
    async fn ack(
        &self,
        db: Data<&DbTxn>,
        _auth: InternalAuth,
        body: Json<Value>,
    ) -> Write::Response<InternalAuth> {
        let r = body.0;
        let (Some(id), Some(generation), Some(ok)) =
            (uuid(&r, "id"), r["generation"].as_i64(), r["ok"].as_bool())
        else {
            return Write::conflict();
        };
        Write::ok(
            cases::value(
                &db,
                "SELECT to_jsonb(moderation_ack($1,$2,$3)) AS value",
                vec![id.into(), generation.into(), ok.into()],
            )
            .await?,
        )
    }
}
fn uuid(value: &Value, key: &str) -> Option<Uuid> {
    value.get(key)?.as_str()?.parse().ok()
}
response!(Read={Ok(200)=>Value,});
response!(Write={Ok(200)=>Value,/// Incomplete command, unavailable target, conflicting replay or stale revision. Reload the case before a new decision.
Conflict(409,error),});
