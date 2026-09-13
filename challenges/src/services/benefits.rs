//! First completion and immutable outbox share the producer transaction.
//! The dispatcher can see only committed rows and retries exact component IDs.
use lib::services::Services;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, DbErr, Statement,
    TransactionTrait,
};
use serde_json::{json, Value};
use std::time::Duration;
use uuid::Uuid;

pub async fn lock_subject(db: &DatabaseTransaction, user: Uuid) -> Result<(), DbErr> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended('challenge-benefit-subject:'||$1::uuid,0))",
        [user.into()],
    ))
    .await?;
    Ok(())
}

pub async fn lock_attempt(db: &DatabaseTransaction, user: Uuid) -> Result<(), DbErr> {
    lock_subject(db, user).await?;
    if db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT 1 FROM moderation_erasure_events WHERE subject=$1",
            [user.into()],
        ))
        .await?
        .is_some()
    {
        return Err(DbErr::Custom("Learning subject was erased".into()));
    }
    Ok(())
}

pub async fn record(
    db: &DatabaseTransaction,
    user: Uuid,
    subtask: Uuid,
    xp: i64,
    configured_coins: i64,
    skills: Vec<String>,
) -> Result<(), DbErr> {
    let earning = Uuid::new_v4();
    // The prospective rule is enforced at the owning producer, even when an
    // old task still contains a configured coin reward. Existing immutable
    // earnings and queued components are untouched and remain deliverable.
    let original = json!({"xp":xp,"coins":0,"configured_coins":configured_coins,"coin_policy":"purchase_only","skills":skills,"description":"Challenges / Aufgaben","credit_note":true});
    let inserted = db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO challenge_benefit_earnings(id,user_id,subtask_id,original) VALUES($1,$2,$3,$4) ON CONFLICT(user_id,subtask_id) DO NOTHING",
        vec![earning.into(),user.into(),subtask.into(),original.into()])).await?;
    if inserted.rows_affected() == 0 {
        return Ok(());
    }
    let mut ordinal = 0_i32;
    if xp != 0 {
        for skill in &skills {
            component(
                db,
                earning,
                user,
                ordinal,
                "xp",
                json!({"skill_id":skill,"xp":xp / skills.len() as i64,"earning_id":earning}),
            )
            .await?;
            ordinal += 1;
        }
    }
    Ok(())
}

async fn component(
    db: &DatabaseTransaction,
    earning: Uuid,
    user: Uuid,
    ordinal: i32,
    kind: &str,
    request: Value,
) -> Result<(), DbErr> {
    db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO challenge_benefit_components(id,earning_id,ordinal,user_id,kind,request) VALUES($1,$2,$3,$4,$5,$6)",
        vec![Uuid::new_v4().into(),earning.into(),ordinal.into(),user.into(),kind.into(),request.into()])).await?;
    Ok(())
}

pub async fn dispatch(db: &DatabaseConnection, services: &Services) -> anyhow::Result<usize> {
    dispatch_with(db, |id, user, kind, request| async move {
        if kind == "xp" {
            Ok(services.skills.apply_benefit(id, user, &request).await?)
        } else {
            Ok(services.shop.apply_benefit(id, user, &request).await?)
        }
    })
    .await
}

async fn dispatch_with<F, Fut>(db: &DatabaseConnection, mut deliver: F) -> anyhow::Result<usize>
where
    F: FnMut(Uuid, Uuid, String, Value) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Value>>,
{
    let mut delivered = 0;
    for _ in 0..100 {
        let tx = db.begin().await?;
        let Some(row)=tx.query_one(Statement::from_string(DbBackend::Postgres,
            "SELECT id,user_id,kind,request FROM challenge_benefit_components WHERE state IN ('pending','uncertain') AND next_attempt_at<=clock_timestamp() ORDER BY next_attempt_at,id LIMIT 1 FOR UPDATE SKIP LOCKED".to_string())).await? else { tx.commit().await?; break; };
        let id: Uuid = row.try_get("", "id")?;
        let user: Uuid = row.try_get("", "user_id")?;
        let kind: String = row.try_get("", "kind")?;
        let request: Value = row.try_get("", "request")?;
        let outcome = deliver(id, user, kind, request).await;
        let (state, receipt) = match outcome {
            Ok(result) if result["state"] == "applied" => {
                delivered += 1;
                ("applied", result)
            }
            Ok(result) if result["state"] == "recipient_erased" || result["state"] == "review" => {
                ("review", result)
            }
            Ok(result) => ("uncertain", result),
            Err(_) => (
                "uncertain",
                json!({"state":"uncertain","reason":"Exact remote outcome unavailable; retry original component"}),
            ),
        };
        tx.execute(Statement::from_sql_and_values(DbBackend::Postgres,
            "UPDATE challenge_benefit_components SET state=$2,receipt=$3,attempts=attempts+1,next_attempt_at=clock_timestamp()+interval '30 seconds' WHERE id=$1",
            vec![id.into(),state.into(),receipt.clone().into()])).await?;
        tx.execute(Statement::from_sql_and_values(DbBackend::Postgres,
            "INSERT INTO challenge_benefit_observations(component_id,attempt,result) SELECT id,attempts,$2 FROM challenge_benefit_components WHERE id=$1",
            vec![id.into(),receipt.into()])).await?;
        tx.commit().await?;
    }
    Ok(delivered)
}

pub async fn run(db: DatabaseConnection, services: Services) {
    loop {
        if let Err(err) = dispatch(&db, &services).await {
            tracing::warn!("Benefit delivery pass remains pending: {err}");
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::Database;

    /// Run explicitly against an empty disposable database. No external service
    /// is called: this checks the real producer, owning commit and visibility.
    #[tokio::test]
    #[ignore = "requires an explicitly supplied disposable PostgreSQL database"]
    async fn owning_producer_transaction() {
        let db = Database::connect(
            std::env::var("BENEFIT_TEST_DATABASE_URL").expect("disposable fixture URL"),
        )
        .await
        .unwrap();
        db.execute_unprepared(include_str!("../../../migration/src/benefit_delivery.sql"))
            .await
            .unwrap();
        let user = Uuid::new_v4();
        let subtask = Uuid::new_v4();
        let read = |user: Uuid| {
            let db = &db;
            async move {
                let tx = db.begin().await.unwrap();
                let value = crate::services::moderation::value(
                    &tx,
                    "SELECT challenge_benefit_export($1) AS value",
                    vec![user.into()],
                )
                .await
                .unwrap();
                tx.commit().await.unwrap();
                value
            }
        };
        let tx = db.begin().await.unwrap();
        lock_subject(&tx, user).await.unwrap();
        record(
            &tx,
            user,
            subtask,
            17,
            5,
            vec!["skill".into(), "skill".into()],
        )
        .await
        .unwrap();
        assert_eq!(read(user).await["earnings"], json!([]));
        tx.rollback().await.unwrap();
        assert_eq!(read(user).await["components"], json!([]));
        let tx = db.begin().await.unwrap();
        lock_subject(&tx, user).await.unwrap();
        record(
            &tx,
            user,
            subtask,
            17,
            5,
            vec!["skill".into(), "skill".into()],
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        let original = read(user).await;
        assert_eq!(original["earnings"].as_array().unwrap().len(), 1);
        assert_eq!(original["components"].as_array().unwrap().len(), 2);
        assert_eq!(original["components"][0]["request"]["xp"], 8);
        assert_eq!(original["components"][1]["request"]["xp"], 8);
        assert_eq!(original["earnings"][0]["original"]["coins"], 0);
        assert_eq!(original["earnings"][0]["original"]["configured_coins"], 5);
        assert!(original["components"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["kind"] == "xp"));
        let tx = db.begin().await.unwrap();
        lock_subject(&tx, user).await.unwrap();
        record(&tx, user, subtask, 999, 999, vec!["changed-skill".into()])
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(read(user).await, original);
        assert_eq!(read(Uuid::new_v4()).await["components"], json!([]));
        // An already earned, still pending historical credit remains payable.
        // Seed its original pre-cutover payload, not a new reward producer call.
        legacy_coin_fixture(&db, user, 5).await;
        // Actual dispatcher transactions, with an idempotent destination stub.
        // One remote reply is lost; the later exact command must not repeat its effect.
        let effects = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::<
            Uuid,
            Value,
        >::new()));
        let lost = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let destination = |id, user, kind, request| {
            let effects = effects.clone();
            let lost = lost.clone();
            async move {
                let mut values = effects.lock().unwrap();
                let exact = json!({"user":user,"kind":kind,"request":request});
                if let Some(old) = values.get(&id) {
                    assert_eq!(old, &exact);
                } else {
                    values.insert(id, exact);
                }
                if !lost.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    anyhow::bail!("Synthetic committed remote reply loss");
                }
                Ok(json!({"state":"applied","operation_id":id}))
            }
        };
        assert_eq!(dispatch_with(&db, destination).await.unwrap(), 2);
        db.execute_unprepared("UPDATE challenge_benefit_components SET next_attempt_at=clock_timestamp() WHERE state='uncertain'").await.unwrap();
        assert_eq!(dispatch_with(&db, destination).await.unwrap(), 1);
        assert_eq!(effects.lock().unwrap().len(), 3);
        assert!(read(user).await["components"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["state"] == "applied"));
        assert_eq!(
            read(user).await["observations"].as_array().unwrap().len(),
            4
        );
        // A local outcome transaction fails after the destination has committed.
        legacy_coin_fixture(&db, user, 7).await;
        db.execute_unprepared("CREATE FUNCTION fixture_reject_benefit_update() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'Synthetic outcome transaction failure'; END $$; CREATE TRIGGER fixture_reject_benefit_update BEFORE UPDATE ON challenge_benefit_components FOR EACH ROW EXECUTE FUNCTION fixture_reject_benefit_update()").await.unwrap();
        assert!(dispatch_with(&db, destination).await.is_err());
        db.execute_unprepared("DROP TRIGGER fixture_reject_benefit_update ON challenge_benefit_components; DROP FUNCTION fixture_reject_benefit_update()").await.unwrap();
        assert_eq!(dispatch_with(&db, destination).await.unwrap(), 1);
        assert_eq!(effects.lock().unwrap().len(), 4);
        db.close().await.unwrap();
    }

    async fn legacy_coin_fixture(db: &DatabaseConnection, user: Uuid, coins: i64) {
        let tx = db.begin().await.unwrap();
        let earning = Uuid::new_v4();
        tx.execute(Statement::from_sql_and_values(DbBackend::Postgres,
            "INSERT INTO challenge_benefit_earnings(id,user_id,subtask_id,original) VALUES($1,$2,$3,$4)",
            vec![earning.into(),user.into(),Uuid::new_v4().into(),json!({"xp":0,"coins":coins,"skills":[],"description":"Challenges / Aufgaben","credit_note":true}).into()])).await.unwrap();
        component(
            &tx,
            earning,
            user,
            0,
            "coins",
            json!({"coins":coins,"description":"Challenges / Aufgaben","credit_note":true}),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
}
