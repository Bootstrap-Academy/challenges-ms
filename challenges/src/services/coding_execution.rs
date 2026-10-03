//! PostgreSQL owns admission, scheduling and the generation fence. No request
//! starts uncommitted work, and no process loads the complete submission history.
use std::collections::HashMap;

use lib::config::CodingExecution;
use schemas::challenges::coding_challenges::QueueStatus;
use sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, DbErr, Statement};
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
    Ok(())
}

/// Caller holds the subject lock, and inserts the submission in this same
/// transaction only after this succeeds. Rejection creates no attempt/outbox.
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
    let row = db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT (SELECT count(*) FROM (SELECT 1 FROM challenges_coding_challenge_submissions s WHERE judge_pending AND NOT EXISTS (SELECT 1 FROM challenges_coding_challenge_result r WHERE r.submission_id=s.id) LIMIT $2) all_pending) < $2 AS global_ok, (SELECT count(*) FROM (SELECT 1 FROM challenges_coding_challenge_submissions s WHERE judge_pending AND creator=$1 AND NOT EXISTS (SELECT 1 FROM challenges_coding_challenge_result r WHERE r.submission_id=s.id) LIMIT $3) user_pending) < $3 AS user_ok",
        [user.into(), i64::from(config.max_pending).into(), i64::from(config.max_pending_per_user).into()])).await?.expect("aggregate row");
    Ok(row.try_get::<bool>("", "global_ok")? && row.try_get::<bool>("", "user_ok")?)
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
