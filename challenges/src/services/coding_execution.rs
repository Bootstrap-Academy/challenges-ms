//! PostgreSQL owns admission, scheduling and the generation fence. No request
//! starts uncommitted work, and no process loads the complete submission history.
use std::{collections::HashMap, time::Duration};

use lib::config::CodingExecution;
use schemas::challenges::coding_challenges::QueueStatus;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, DbErr, Statement,
    TransactionTrait,
};
use tracing::error;
use uuid::Uuid;

#[derive(Clone, Copy, Debug)]
pub struct Claim {
    pub submission: Uuid,
    pub user: Uuid,
    pub owner: Uuid,
    pub generation: i64,
}

pub fn validate(config: &CodingExecution, concurrency: usize) -> anyhow::Result<()> {
    anyhow::ensure!(
        concurrency > 0 && concurrency <= 1024,
        "coding max_concurrency must be 1..=1024"
    );
    anyhow::ensure!(
        config.max_pending > 0 && config.max_pending_per_user > 0,
        "coding pending limits must be positive"
    );
    anyhow::ensure!(
        (6..=3600).contains(&config.lease_seconds),
        "coding lease_seconds must be 6..=3600"
    );
    anyhow::ensure!(
        (10..=60000).contains(&config.poll_milliseconds),
        "coding poll_milliseconds must be 10..=60000"
    );
    anyhow::ensure!(
        config.retry_seconds > 0 && config.max_execution_seconds > 0,
        "coding retry and execution timeouts must be positive"
    );
    anyhow::ensure!(
        (1..=20).contains(&config.max_technical_attempts),
        "coding max_technical_attempts must be 1..=20"
    );
    Ok(())
}

/// Whether this claim may still run. A claim beyond the cap follows only
/// technical failures, including workers that lost their lease or crashed.
pub fn attempt_allowed(config: &CodingExecution, claim: Claim) -> bool {
    claim.generation <= i64::from(config.max_technical_attempts)
}

/// Whether a technical failure of this claim used up the last attempt.
pub fn last_attempt(config: &CodingExecution, claim: Claim) -> bool {
    claim.generation >= i64::from(config.max_technical_attempts)
}

/// Exponential backoff gives a full artifact cache or a restarting sandbox
/// time to recover, without retrying a persistent failure every few seconds.
pub fn retry_delay(config: &CodingExecution, claim: Claim) -> u32 {
    let doublings = claim.generation.clamp(1, 7) - 1;
    config.retry_seconds.saturating_mul(1 << doublings)
}

/// Caller holds the subject lock, and inserts a submission or inline lease in
/// this same transaction only after this succeeds. Both share the exact caps.
/// Rejection creates no attempt/outbox, and expired inline leases cost no slots.
pub async fn admit(
    db: &DatabaseTransaction,
    user: Uuid,
    config: &CodingExecution,
) -> Result<bool, DbErr> {
    db.execute(Statement::from_string(
        DbBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended('coding-execution-admission',0))",
    ))
    .await?;
    prune_inline(db).await?;
    let row = db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT (SELECT count(*) FROM (SELECT 1 FROM (SELECT 1 FROM challenges_coding_challenge_submissions s WHERE judge_pending AND NOT EXISTS (SELECT 1 FROM challenges_coding_challenge_result r WHERE r.submission_id=s.id) UNION ALL SELECT 1 FROM challenge_coding_inline_runs WHERE expires_at > clock_timestamp()) pending LIMIT $2) all_pending) < $2 AS global_ok, (SELECT count(*) FROM (SELECT 1 FROM (SELECT 1 FROM challenges_coding_challenge_submissions s WHERE judge_pending AND creator=$1 AND NOT EXISTS (SELECT 1 FROM challenges_coding_challenge_result r WHERE r.submission_id=s.id) UNION ALL SELECT 1 FROM challenge_coding_inline_runs WHERE user_id=$1 AND expires_at > clock_timestamp()) pending LIMIT $3) user_pending) < $3 AS user_ok",
        [user.into(), i64::from(config.max_pending).into(), i64::from(config.max_pending_per_user).into()])).await?.expect("aggregate row");
    Ok(row.try_get::<bool>("", "global_ok")? && row.try_get::<bool>("", "user_ok")?)
}

/// Owns transient admission independently of the long HTTP transaction. Drop
/// releases on timeout/client cancellation; expiry also covers API process loss.
pub struct InlineRun {
    id: Option<Uuid>,
    db: DatabaseConnection,
    cleanup_timeout: Duration,
}

impl InlineRun {
    pub async fn reserve(
        db: &DatabaseConnection,
        user: Uuid,
        subtask: Uuid,
        config: &CodingExecution,
    ) -> Result<Option<Self>, DbErr> {
        let transaction = db.begin().await?;
        // Same lock order as submission admission and account erasure.
        super::benefits::lock_attempt(&transaction, user).await?;
        if !admit(&transaction, user, config).await? {
            return Ok(None);
        }
        let id = Uuid::new_v4();
        let guard = Self {
            id: Some(id),
            db: db.clone(),
            cleanup_timeout: Duration::from_secs(u64::from(config.lease_seconds)),
        };
        transaction.execute(Statement::from_sql_and_values(DbBackend::Postgres,
            "INSERT INTO challenge_coding_inline_runs(id,user_id,subtask_id,expires_at) VALUES($1,$2,$3,clock_timestamp()+$4::bigint * interval '1 second')",
            [id.into(), user.into(), subtask.into(), i64::from(config.max_execution_seconds).into()])).await?;
        transaction.commit().await?;
        Ok(Some(guard))
    }

    pub async fn release(&mut self) -> Result<(), DbErr> {
        if let Some(id) = self.id {
            release_inline(&self.db, id).await?;
            self.id = None;
        }
        Ok(())
    }

    /// Erasure/task deletion removes the row. Stop the local executor future
    /// when that happens rather than continuing with an erased subject.
    pub async fn until_removed(&self, poll_milliseconds: u32) -> Result<(), DbErr> {
        let id = self.id.expect("active inline lease");
        loop {
            let active = self.db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
                "SELECT id FROM challenge_coding_inline_runs WHERE id=$1 AND expires_at > clock_timestamp()",
                [id.into()])).await?.is_some();
            if !active {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(u64::from(poll_milliseconds))).await;
        }
    }
}

impl Drop for InlineRun {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            let db = self.db.clone();
            let timeout = self.cleanup_timeout;
            // The HTTP future can disappear at any await point. Cleanup owns
            // only this UUID; an expired lease remains harmless if DB is down.
            tokio::spawn(async move {
                match tokio::time::timeout(timeout, release_inline(&db, id)).await {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => error!("could not release inline coding test {id}: {err}"),
                    Err(_) => error!("inline coding cleanup timed out for {id}"),
                }
            });
        }
    }
}

async fn release_inline(db: &DatabaseConnection, id: Uuid) -> Result<(), DbErr> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "DELETE FROM challenge_coding_inline_runs WHERE id=$1",
        [id.into()],
    ))
    .await?;
    Ok(())
}

async fn prune_inline(db: &impl ConnectionTrait) -> Result<(), DbErr> {
    db.execute(Statement::from_string(
        DbBackend::Postgres,
        "DELETE FROM challenge_coding_inline_runs WHERE expires_at <= clock_timestamp()",
    ))
    .await?;
    Ok(())
}

pub async fn erase_inline(db: &DatabaseTransaction, user: Uuid) -> Result<u64, DbErr> {
    Ok(db
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "DELETE FROM challenge_coding_inline_runs WHERE user_id=$1",
            [user.into()],
        ))
        .await?
        .rows_affected())
}

pub async fn claim(
    db: &impl ConnectionTrait,
    owner: Uuid,
    lease_seconds: u32,
) -> Result<Option<Claim>, DbErr> {
    db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
        "WITH candidate AS (SELECT s.id FROM challenges_coding_challenge_submissions s WHERE judge_pending AND judge_available_at <= clock_timestamp() AND (judge_lease_until IS NULL OR judge_lease_until <= clock_timestamp()) AND NOT EXISTS (SELECT 1 FROM challenges_coding_challenge_result r WHERE r.submission_id=s.id) ORDER BY judge_available_at, creation_timestamp, id LIMIT 1 FOR UPDATE OF s SKIP LOCKED) UPDATE challenges_coding_challenge_submissions s SET judge_generation=judge_generation+1, judge_lease_owner=$1, judge_lease_until=clock_timestamp()+$2::bigint * interval '1 second' FROM candidate WHERE s.id=candidate.id RETURNING s.id, s.creator, s.judge_generation",
        [owner.into(), i64::from(lease_seconds).into()])).await?.map(|row| Ok(Claim {
            submission: row.try_get("", "id")?, user: row.try_get("", "creator")?, owner,
            generation: row.try_get("", "judge_generation")?,
        })).transpose()
}

pub async fn renew(
    db: &impl ConnectionTrait,
    claim: Claim,
    lease_seconds: u32,
) -> Result<bool, DbErr> {
    Ok(db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "UPDATE challenges_coding_challenge_submissions SET judge_lease_until=clock_timestamp()+$4::bigint * interval '1 second' WHERE id=$1 AND judge_generation=$2 AND judge_lease_owner=$3 AND judge_pending AND judge_lease_until > clock_timestamp()",
        [claim.submission.into(), claim.generation.into(), claim.owner.into(), i64::from(lease_seconds).into()])).await?.rows_affected() == 1)
}

/// Lock order is subject, then submission (also used by account erasure).
/// Holding this row until the result/outbox commit prevents a newer generation
/// from being claimed while the successful fence is being consumed.
pub async fn fence(db: &DatabaseTransaction, claim: Claim) -> Result<bool, DbErr> {
    Ok(db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT id FROM challenges_coding_challenge_submissions WHERE id=$1 AND judge_generation=$2 AND judge_lease_owner=$3 AND judge_pending AND judge_lease_until > clock_timestamp() FOR UPDATE",
        [claim.submission.into(), claim.generation.into(), claim.owner.into()])).await?.is_some())
}

pub async fn complete(db: &DatabaseTransaction, claim: Claim) -> Result<(), DbErr> {
    db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "UPDATE challenges_coding_challenge_submissions SET judge_pending=false, judge_lease_owner=NULL, judge_lease_until=NULL WHERE id=$1 AND judge_generation=$2 AND judge_lease_owner=$3",
        [claim.submission.into(), claim.generation.into(), claim.owner.into()])).await?;
    Ok(())
}

/// Technical failures remain pending and cost no hearts. A stale worker cannot
/// release, postpone or overwrite a replacement's claim.
pub async fn retry(
    db: &impl ConnectionTrait,
    claim: Claim,
    delay_seconds: u32,
) -> Result<(), DbErr> {
    db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "UPDATE challenges_coding_challenge_submissions SET judge_lease_owner=NULL, judge_lease_until=NULL, judge_available_at=clock_timestamp()+$4::bigint * interval '1 second' WHERE id=$1 AND judge_generation=$2 AND judge_lease_owner=$3 AND judge_pending AND judge_lease_until > clock_timestamp()",
        [claim.submission.into(), claim.generation.into(), claim.owner.into(), i64::from(delay_seconds).into()])).await?;
    Ok(())
}

/// Close a submission whose attempts all failed technically. It stays without
/// a result, so it records no attempt, heart operation, XP or further lesson
/// start, and it frees its pending slot. `technical_failure` reports it.
/// The same fence as `retry` keeps a stale worker from closing a newer claim.
pub async fn abandon(db: &impl ConnectionTrait, claim: Claim) -> Result<bool, DbErr> {
    Ok(db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "UPDATE challenges_coding_challenge_submissions s SET judge_pending=false, judge_lease_owner=NULL, judge_lease_until=NULL WHERE id=$1 AND judge_generation=$2 AND judge_lease_owner=$3 AND judge_pending AND judge_lease_until > clock_timestamp() AND NOT EXISTS (SELECT 1 FROM challenges_coding_challenge_result r WHERE r.submission_id=s.id)",
        [claim.submission.into(), claim.generation.into(), claim.owner.into()])).await?.rows_affected() == 1)
}

pub async fn advertise(
    db: &impl ConnectionTrait,
    owner: Uuid,
    capacity: usize,
    lease_seconds: u32,
) -> Result<(), DbErr> {
    db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO challenge_coding_workers(id,capacity,lease_until) VALUES($1,$2,clock_timestamp()+$3::bigint * interval '1 second') ON CONFLICT(id) DO UPDATE SET capacity=excluded.capacity, lease_until=excluded.lease_until",
        [owner.into(), (capacity as i32).into(), i64::from(lease_seconds).into()])).await?;
    db.execute(Statement::from_string(
        DbBackend::Postgres,
        "DELETE FROM challenge_coding_workers WHERE lease_until <= clock_timestamp()",
    ))
    .await?;
    // Reuse the worker's existing heartbeat to remove leases left by an API
    // process crash, even when no new examples/submissions are being admitted.
    prune_inline(db).await?;
    Ok(())
}

pub async fn status(db: &impl ConnectionTrait) -> Result<QueueStatus, DbErr> {
    let row = db.query_one(Statement::from_string(DbBackend::Postgres,
        "SELECT (SELECT coalesce(sum(capacity),0)::bigint FROM challenge_coding_workers WHERE lease_until > clock_timestamp()) AS workers, count(*) FILTER (WHERE judge_lease_until > clock_timestamp()) AS active, count(*) FILTER (WHERE judge_lease_until IS NULL OR judge_lease_until <= clock_timestamp()) AS waiting FROM challenges_coding_challenge_submissions s WHERE judge_pending AND NOT EXISTS (SELECT 1 FROM challenges_coding_challenge_result r WHERE r.submission_id=s.id)")).await?.expect("aggregate row");
    Ok(QueueStatus {
        workers: row.try_get::<i64>("", "workers")? as usize,
        active: row.try_get::<i64>("", "active")? as usize,
        waiting: row.try_get::<i64>("", "waiting")? as usize,
    })
}

pub async fn positions(
    db: &impl ConnectionTrait,
    user: Uuid,
) -> Result<HashMap<Uuid, usize>, DbErr> {
    db.query_all(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT id, position FROM (SELECT id, creator, CASE WHEN judge_lease_until > clock_timestamp() THEN 0 ELSE count(*) FILTER (WHERE judge_lease_until IS NULL OR judge_lease_until <= clock_timestamp()) OVER (ORDER BY judge_available_at, creation_timestamp, id) END AS position FROM challenges_coding_challenge_submissions s WHERE judge_pending AND NOT EXISTS (SELECT 1 FROM challenges_coding_challenge_result r WHERE r.submission_id=s.id)) pending WHERE creator=$1",
        [user.into()])).await?.into_iter().map(|row| Ok((row.try_get("", "id")?, row.try_get::<i64>("", "position")? as usize))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(generation: i64) -> Claim {
        Claim {
            submission: Uuid::nil(),
            user: Uuid::nil(),
            owner: Uuid::nil(),
            generation,
        }
    }

    #[test]
    fn technical_attempts_are_capped_with_backoff() {
        let config = CodingExecution {
            retry_seconds: 10,
            max_technical_attempts: 3,
            ..Default::default()
        };
        assert!((1..=3).all(|generation| attempt_allowed(&config, claim(generation))));
        assert!(!attempt_allowed(&config, claim(4)));
        assert!(!last_attempt(&config, claim(2)));
        assert!(last_attempt(&config, claim(3)) && last_attempt(&config, claim(9)));
        let delays: Vec<_> = (1..=9).map(|g| retry_delay(&config, claim(g))).collect();
        assert_eq!(delays, [10, 20, 40, 80, 160, 320, 640, 640, 640]);
        assert!(validate(&config, 1).is_ok());
        for attempts in [0, 21] {
            let config = CodingExecution {
                max_technical_attempts: attempts,
                ..Default::default()
            };
            assert!(validate(&config, 1).is_err());
        }
        assert!(validate(&CodingExecution::default(), 1).is_ok());
    }
}
