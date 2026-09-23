//! XP for lesson units that skills-ms has completed and verified itself.
//! The first report per user and unit records the earning in the same
//! transaction; the existing benefit dispatcher delivers it to `skills_xp`.
//! No heart operation and no coins are ever produced here.
use chrono::{DateTime, Utc};
use schemas::challenges::lesson_milestones::{LessonMilestone, LessonMilestoneCompletion};
use sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, DbErr, QueryResult, Statement};
use uuid::Uuid;

pub enum Recorded {
    Created(LessonMilestone),
    Existing(LessonMilestone),
    SubjectErased,
}

pub async fn record(
    db: &DatabaseTransaction,
    user: Uuid,
    unit_id: &str,
    skill_id: &str,
    xp: u64,
    completion: LessonMilestoneCompletion,
) -> Result<Recorded, DbErr> {
    // Same lock order as attempts and account erasure: subject first.
    super::benefits::lock_subject(db, user).await?;
    if super::benefits::erased(db, user).await? {
        return Ok(Recorded::SubjectErased);
    }
    let xp = i64::try_from(xp).map_err(|_| DbErr::Custom("XP out of range".into()))?;
    let inserted = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO challenge_lesson_milestones(id,user_id,unit_id,skill_id,xp,completion) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(user_id,unit_id) DO NOTHING RETURNING id,unit_id,skill_id,xp,completion,completed_at",
            vec![
                Uuid::new_v4().into(),
                user.into(),
                unit_id.into(),
                skill_id.into(),
                xp.into(),
                completion.as_str().into(),
            ],
        ))
        .await?;
    let Some(row) = inserted else {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT id,unit_id,skill_id,xp,completion,completed_at FROM challenge_lesson_milestones WHERE user_id=$1 AND unit_id=$2",
                [user.into(), unit_id.into()],
            ))
            .await?
            .ok_or_else(|| DbErr::Custom("Lesson milestone vanished under lock".into()))?;
        return Ok(Recorded::Existing(milestone(&row)?));
    };
    let id: Uuid = row.try_get("", "id")?;
    super::benefits::record(db, user, id, xp, 0, vec![skill_id.to_owned()]).await?;
    Ok(Recorded::Created(milestone(&row)?))
}

fn milestone(row: &QueryResult) -> Result<LessonMilestone, DbErr> {
    let completion: String = row.try_get("", "completion")?;
    let xp: i64 = row.try_get("", "xp")?;
    let completed_at: DateTime<Utc> = row.try_get("", "completed_at")?;
    Ok(LessonMilestone {
        unit_id: row.try_get("", "unit_id")?,
        skill_id: row.try_get("", "skill_id")?,
        xp: u64::try_from(xp).map_err(|_| DbErr::Custom("Negative milestone XP".into()))?,
        completion: LessonMilestoneCompletion::parse(&completion)
            .ok_or_else(|| DbErr::Custom("Unknown milestone completion".into()))?,
        completed_at,
    })
}

pub async fn export(db: &DatabaseTransaction, user: Uuid) -> Result<Vec<LessonMilestone>, DbErr> {
    db.query_all(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT id,unit_id,skill_id,xp,completion,completed_at FROM challenge_lesson_milestones WHERE user_id=$1 ORDER BY completed_at,id",
        [user.into()],
    ))
    .await?
    .iter()
    .map(milestone)
    .collect()
}

pub async fn delete(db: &DatabaseTransaction, user: Uuid) -> Result<u64, DbErr> {
    Ok(db
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "DELETE FROM challenge_lesson_milestones WHERE user_id=$1",
            [user.into()],
        ))
        .await?
        .rows_affected())
}
