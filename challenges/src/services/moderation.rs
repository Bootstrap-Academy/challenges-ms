//! Owning-service case operations. All functions use the request transaction;
//! state, statement and durable delivery work commit or roll back together.
use entity::{challenges_subtasks, sea_orm_active_enums::ChallengesReportReason};
use poem_openapi::types::ToJSON;
use sea_orm::{
    ConnectionTrait, DatabaseTransaction, DbBackend, DbErr, EntityTrait, Statement,
    Value as SqlValue,
};
use serde_json::{json, Value};
use uuid::Uuid;

pub const REDRESS: &str = "Du kannst ab dieser Mitteilung mindestens sechs Kalendermonate lang kostenlos eine Überprüfung anfordern: /moderation oder hallo@bootstrap.academy. Dabei kannst du Fehler erklären oder neue Informationen ergänzen. Deine Beschwerde wird nicht allein automatisch entschieden. Andere Beschwerdewege und der Rechtsweg bleiben offen.";
pub const SUBTASK_SCOPE: &str = "Diese Teilaufgabe auf Bootstrap Academy";

pub async fn value(
    db: &DatabaseTransaction,
    sql: &str,
    args: Vec<SqlValue>,
) -> Result<Value, DbErr> {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Postgres,
        sql,
        args,
    ))
    .await?
    .ok_or_else(|| DbErr::Custom("Moderation query returned no result".into()))?
    .try_get("", "value")
}

pub async fn lock_subtask(db: &DatabaseTransaction, id: Uuid) -> Result<(), DbErr> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended('moderation:subtask:'||$1::uuid,0))",
        [id.into()],
    ))
    .await?;
    Ok(())
}

pub async fn open_subtask(
    db: &DatabaseTransaction,
    id: Uuid,
    actor: Option<Uuid>,
    subtask: &challenges_subtasks::Model,
    source: &str,
    notifier: Option<Uuid>,
    mut evidence: Value,
) -> Result<(), DbErr> {
    lock_subtask(db, subtask.id).await?;
    let content =
        super::authored_export::subtask_content_for(db, subtask.creator, Some(subtask.id))
            .await?
            .remove(&subtask.id)
            .ok_or_else(super::authored_export::inconsistent_content)?;
    if content.subtask_type() != subtask.ty {
        return Err(super::authored_export::inconsistent_content());
    }
    // Never snapshot a sibling selected by the shared parent task_id.
    evidence["target_content"] = content
        .to_json()
        .ok_or_else(super::authored_export::inconsistent_content)?;
    evidence["task_id"] = json!(subtask.task_id);
    evidence["content_revision"] = value(db,"SELECT to_jsonb(coalesce((SELECT content_revision FROM moderation_targets WHERE kind='subtask' AND id=$1),0)) AS value",vec![subtask.id.into()]).await?;
    value(
        db,
        "SELECT to_jsonb(moderation_open($1,$2,'subtask',$3,$4,$5,$6,$7)) AS value",
        vec![
            id.into(),
            actor.into(),
            subtask.id.into(),
            subtask.creator.into(),
            source.into(),
            notifier.into(),
            evidence.into(),
        ],
    )
    .await?;
    Ok(())
}

pub async fn decide(
    db: &DatabaseTransaction,
    actor: Uuid,
    mut command: Value,
) -> Result<Value, DbErr> {
    if let Some(case) = command["case_id"]
        .as_str()
        .and_then(|v| v.parse::<Uuid>().ok())
    {
        let target=value(db,"SELECT coalesce((SELECT jsonb_build_object('kind',target_kind,'id',target_id,'subject',subject) FROM moderation_cases WHERE id=$1),'null') AS value",vec![case.into()]).await?;
        if target["kind"] == "subtask" {
            let id: Uuid = serde_json::from_value(target["id"].clone())
                .map_err(|_| DbErr::Custom("Target unavailable".into()))?;
            lock_subtask(db, id).await?;
            let subject: Uuid = serde_json::from_value(target["subject"].clone())
                .map_err(|_| DbErr::Custom("Subject unavailable".into()))?;
            if let Some(content) =
                super::authored_export::subtask_content_for(db, subject, Some(id))
                    .await?
                    .remove(&id)
            {
                command["reviewed_content"] = content
                    .to_json()
                    .ok_or_else(super::authored_export::inconsistent_content)?;
            }
        }
    }
    value(
        db,
        "SELECT moderation_decide($1,$2) AS value",
        vec![actor.into(), command.into()],
    )
    .await
}

pub async fn report(
    db: &DatabaseTransaction,
    id: Uuid,
    notifier: Option<Uuid>,
    subtask: &challenges_subtasks::Model,
    reason: ChallengesReportReason,
    private_comment: &str,
    basis: Value,
) -> Result<(), DbErr> {
    let automatic = reason == ChallengesReportReason::Dislike;
    let source = if automatic {
        "rating_threshold"
    } else {
        "user_report"
    };
    let ratings=value(db,"SELECT jsonb_build_object('positive',count(*) FILTER(WHERE rating='positive'),'negative',count(*) FILTER(WHERE rating='negative'),'cohort',md5(coalesce(jsonb_agg(jsonb_build_array(user_id,rating) ORDER BY user_id) FILTER(WHERE rating IS NOT NULL),'[]')::text)) AS value FROM challenges_user_subtasks WHERE subtask_id=$1",vec![subtask.id.into()]).await?;
    let may_apply_automatically = basis["automatic_quality_basis_confirmed"] == true;
    open_subtask(db,id,notifier,subtask,source,notifier,
        json!({"reason":format!("{reason:?}"),"comment":private_comment,"urgent_triage": reason==ChallengesReportReason::Abuse,"rule_evidence":basis,"author_contact":basis["contact"],"rating_counts":ratings})).await?;
    let rationale = match reason {
        ChallengesReportReason::Wrong => Some("Die Aufgabe oder ihre Lösung wurde als fachlich falsch gemeldet. Das ist noch keine abschließende Bewertung.".to_owned()),
        ChallengesReportReason::UnrelatedSkill => Some("Die Aufgabe wurde gemeldet, weil sie nicht zur zugeordneten Fähigkeit passen soll. Das ist noch keine abschließende Bewertung.".to_owned()),
        ChallengesReportReason::Dislike => Some(format!("Die Aufgabe hat {} negative und {} positive Bewertungen. Damit ist die Grenze für eine automatische Qualitätsprüfung erreicht: mindestens 10 negative und mehr negative als positive Bewertungen.",ratings["negative"],ratings["positive"])),
        _ => None,
    };
    // A free-text allegation cannot safely be turned into a finding. Keep it in
    // the human (including urgent) queue without an unexplained auto-restriction.
    if let Some(rationale) = rationale.filter(|_| may_apply_automatically) {
        decide(db,Uuid::nil(),json!({"request_key":id,"case_id":id,"expected_revision":0,"outcome":"provisional",
            "rationale":rationale,"notifier_rationale":"Die gemeldete Aufgabe ist bis zur Überprüfung vorläufig ausgeblendet.",
            "ground":"Qualitätsprüfung nach AGB 14.3 und 14.4",
            "rule_version":basis["rule_identity"],"automation":"Automatisch vorläufig ausgeblendet; die Überprüfung ist noch offen.",
            "reviewed_content_revision":value(db,"SELECT to_jsonb(content_revision) AS value FROM moderation_targets WHERE kind='subtask' AND id=$1",vec![subtask.id.into()]).await?,"scope":SUBTASK_SCOPE,"redress":REDRESS})).await?;
    }
    Ok(())
}

pub async fn inbox(db: &DatabaseTransaction, user: Uuid) -> Result<Value, DbErr> {
    value(
        db,
        "SELECT moderation_inbox($1) AS value",
        vec![user.into()],
    )
    .await
}

pub async fn review_target(db: &DatabaseTransaction, case: &Value) -> Result<Value, DbErr> {
    if case["target_kind"] != "subtask" {
        return Ok(Value::Null);
    }
    let id: Uuid = serde_json::from_value(case["target_id"].clone())
        .map_err(|_| DbErr::Custom("Target unavailable".into()))?;
    let subject: Uuid = serde_json::from_value(case["subject"].clone())
        .map_err(|_| DbErr::Custom("Subject unavailable".into()))?;
    lock_subtask(db, id).await?;
    let state=value(db,"SELECT jsonb_build_object('revision',content_revision,'withdrawn',withdrawn) AS value FROM moderation_targets WHERE kind='subtask' AND id=$1",vec![id.into()]).await?;
    let content = super::authored_export::subtask_content_for(db, subject, Some(id))
        .await?
        .remove(&id)
        .and_then(|c| c.to_json());
    Ok(json!({"revision":state["revision"],"withdrawn":state["withdrawn"],"content":content}))
}

pub async fn erasure_marker(db: &DatabaseTransaction, user: Uuid) -> Result<(), DbErr> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT set_config('academy.moderation_erasure_subject',$1,true)",
        [user.to_string().into()],
    ))
    .await?;
    Ok(())
}

pub async fn reload_subtask(
    db: &DatabaseTransaction,
    id: Uuid,
) -> Result<challenges_subtasks::Model, DbErr> {
    let row = challenges_subtasks::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| DbErr::Custom("Subtask disappeared during moderation".into()))?;
    effective_subtask(db, row).await
}

/// Read-only projection: finite visibility measures cease at their stored end,
/// independently of the later worker that appends expiry notices/history.
/// This also works in T11's READ ONLY transaction and never creates targets.
pub async fn effective_subtasks(
    db: &DatabaseTransaction,
    mut rows: Vec<challenges_subtasks::Model>,
) -> Result<Vec<challenges_subtasks::Model>, DbErr> {
    if rows.is_empty() {
        return Ok(rows);
    }
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let states=value(db,"SELECT coalesce(jsonb_object_agg(id,moderation_effect('subtask',id)),'{}') AS value FROM moderation_targets WHERE kind='subtask' AND id=ANY($1)",vec![ids.into()]).await?;
    for row in &mut rows {
        if let Some(state) = states.get(row.id.to_string()) {
            row.enabled = state["enabled"]
                .as_bool()
                .ok_or_else(|| DbErr::Custom("Invalid moderation state".into()))?;
            row.retired = state["retired"]
                .as_bool()
                .ok_or_else(|| DbErr::Custom("Invalid moderation state".into()))?;
            row.moderation_removed = state["removed"]
                .as_bool()
                .ok_or_else(|| DbErr::Custom("Invalid moderation state".into()))?;
        }
    }
    Ok(rows)
}
pub async fn effective_subtask(
    db: &DatabaseTransaction,
    row: challenges_subtasks::Model,
) -> Result<challenges_subtasks::Model, DbErr> {
    Ok(effective_subtasks(db, vec![row]).await?.remove(0))
}
