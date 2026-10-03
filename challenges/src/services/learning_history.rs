//! Local participation evidence for Skills admission. Never call access checks
//! here: Skills may be asking while admitting a direct Challenges request.
use schemas::challenges::learning_history::{
    LearningHistory, LearningHistoryRequest, LectureBinding,
};
use sea_orm::{ConnectionTrait, DbBackend, DbErr, Statement};
use uuid::Uuid;

pub async fn lookup(
    db: &impl ConnectionTrait,
    user: Uuid,
    request: LearningHistoryRequest,
) -> Result<LearningHistory, DbErr> {
    let mut history = LearningHistory::default();
    if request.subtask_ids.is_empty() && request.lecture_bindings.is_empty() {
        return Ok(history);
    }
    let (courses, lectures): (Vec<_>, Vec<_>) = request
        .lecture_bindings
        .into_iter()
        .map(|binding| (binding.course_id, binding.lecture_id))
        .unzip();

    // One snapshot, bounded to the requested tasks/lectures. A rating or an
    // otherwise empty UserSubtask row alone does not prove participation.
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"
WITH requested_subtasks AS (
    SELECT DISTINCT unnest($2::uuid[]) AS subtask_id
), requested_lectures AS (
    SELECT DISTINCT * FROM unnest($3::text[], $4::text[]) AS r(course_id, lecture_id)
), candidates AS (
    SELECT subtask_id FROM requested_subtasks
    UNION
    SELECT s.id FROM challenges_subtasks s
    JOIN challenges_course_tasks ct ON ct.task_id = s.task_id
    JOIN requested_lectures r ON r.course_id = ct.course_id AND r.lecture_id = ct.lecture_id
), attempted AS (
    SELECT c.subtask_id FROM candidates c
    WHERE EXISTS (
        SELECT 1 FROM challenges_user_subtasks p
        WHERE p.user_id = $1 AND p.subtask_id = c.subtask_id
        AND (p.attempts > 0 OR p.last_attempt_timestamp IS NOT NULL OR p.solved_timestamp IS NOT NULL)
    ) OR EXISTS (
        SELECT 1 FROM challenges_multiple_choice_attempts a
        WHERE a.user_id = $1 AND a.question_id = c.subtask_id
    ) OR EXISTS (
        SELECT 1 FROM challenges_matching_attempts a
        WHERE a.user_id = $1 AND a.matching_id = c.subtask_id
    ) OR EXISTS (
        SELECT 1 FROM challenges_question_attempts a
        WHERE a.user_id = $1 AND a.question_id = c.subtask_id
    ) OR EXISTS (
        SELECT 1 FROM challenges_coding_challenge_submissions s
        WHERE s.creator = $1 AND s.subtask_id = c.subtask_id
    )
)
SELECT a.subtask_id, NULL::text AS course_id, NULL::text AS lecture_id
FROM attempted a JOIN requested_subtasks r USING (subtask_id)
UNION ALL
SELECT NULL::uuid, r.course_id, r.lecture_id FROM requested_lectures r
WHERE EXISTS (
    SELECT 1 FROM challenges_course_tasks ct
    JOIN challenges_subtasks s ON s.task_id = ct.task_id
    JOIN attempted a ON a.subtask_id = s.id
    WHERE ct.course_id = r.course_id AND ct.lecture_id = r.lecture_id
)
ORDER BY subtask_id, course_id, lecture_id
"#,
            [
                user.into(),
                request.subtask_ids.into(),
                courses.into(),
                lectures.into(),
            ],
        ))
        .await?;
    for row in rows {
        if let Some(id) = row.try_get::<Option<Uuid>>("", "subtask_id")? {
            history.attempted_subtask_ids.push(id);
        } else {
            history.attempted_lecture_bindings.push(LectureBinding {
                course_id: row.try_get("", "course_id")?,
                lecture_id: row.try_get("", "lecture_id")?,
            });
        }
    }
    Ok(history)
}
