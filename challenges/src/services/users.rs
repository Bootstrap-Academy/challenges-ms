use std::collections::HashSet;

use entity::{
    challenges_ban, challenges_coding_challenge_submissions, challenges_matching_attempts,
    challenges_multiple_choice_attempts, challenges_question_attempts, challenges_subtask_reports,
    challenges_subtasks, challenges_tasks, challenges_user_subtasks,
};
use sea_orm::{
    sea_query::{Alias, Expr, IntoColumnRef, IntoTableRef, Query, SelectStatement, UnionType},
    ColumnTrait, Condition, ConnectionTrait, DatabaseTransaction, DbErr, EntityTrait, Order,
    QueryFilter, QuerySelect,
};
use uuid::Uuid;

/// Delete all rows that belong to a user.
///
/// Subtasks created by the user are deleted as well, including everything that
/// references them (such as attempts and submissions of other users), which the
/// database takes care of via `ON DELETE CASCADE`. Tasks created by the user
/// are shared between all users, so they are only deleted if no subtask is left
/// in them.
///
/// Returns the number of rows that have been deleted directly.
pub async fn delete_user_data(db: &DatabaseTransaction, user_id: Uuid) -> Result<u64, DbErr> {
    let mut rows = 0;

    rows += challenges_ban::Entity::delete_many()
        .filter(
            Condition::any()
                .add(challenges_ban::Column::UserId.eq(user_id))
                .add(challenges_ban::Column::Creator.eq(user_id)),
        )
        .exec(db)
        .await?
        .rows_affected;

    rows += challenges_subtask_reports::Entity::delete_many()
        .filter(challenges_subtask_reports::Column::UserId.eq(user_id))
        .exec(db)
        .await?
        .rows_affected;

    rows += challenges_multiple_choice_attempts::Entity::delete_many()
        .filter(challenges_multiple_choice_attempts::Column::UserId.eq(user_id))
        .exec(db)
        .await?
        .rows_affected;

    rows += challenges_question_attempts::Entity::delete_many()
        .filter(challenges_question_attempts::Column::UserId.eq(user_id))
        .exec(db)
        .await?
        .rows_affected;

    rows += challenges_matching_attempts::Entity::delete_many()
        .filter(challenges_matching_attempts::Column::UserId.eq(user_id))
        .exec(db)
        .await?
        .rows_affected;

    rows += challenges_coding_challenge_submissions::Entity::delete_many()
        .filter(challenges_coding_challenge_submissions::Column::Creator.eq(user_id))
        .exec(db)
        .await?
        .rows_affected;

    rows += challenges_user_subtasks::Entity::delete_many()
        .filter(challenges_user_subtasks::Column::UserId.eq(user_id))
        .exec(db)
        .await?
        .rows_affected;

    rows += challenges_subtasks::Entity::delete_many()
        .filter(challenges_subtasks::Column::Creator.eq(user_id))
        .exec(db)
        .await?
        .rows_affected;

    rows += delete_empty_tasks(db, user_id).await?;

    Ok(rows)
}

/// Delete all tasks created by a user that do not contain any subtasks anymore.
async fn delete_empty_tasks(db: &DatabaseTransaction, user_id: Uuid) -> Result<u64, DbErr> {
    let task_ids: Vec<Uuid> = challenges_tasks::Entity::find()
        .select_only()
        .column(challenges_tasks::Column::Id)
        .filter(challenges_tasks::Column::Creator.eq(user_id))
        .into_tuple()
        .all(db)
        .await?;
    if task_ids.is_empty() {
        return Ok(0);
    }

    let used: HashSet<Uuid> = challenges_subtasks::Entity::find()
        .select_only()
        .column(challenges_subtasks::Column::TaskId)
        .filter(challenges_subtasks::Column::TaskId.is_in(task_ids.iter().copied()))
        .into_tuple()
        .all(db)
        .await?
        .into_iter()
        .collect();
    let empty = task_ids
        .into_iter()
        .filter(|task_id| !used.contains(task_id))
        .collect::<Vec<_>>();
    if empty.is_empty() {
        return Ok(0);
    }

    Ok(challenges_tasks::Entity::delete_many()
        .filter(challenges_tasks::Column::Id.is_in(empty))
        .exec(db)
        .await?
        .rows_affected)
}

/// Return the distinct user ids that are referenced anywhere in the challenges
/// database, starting after `after` and limited to `limit` ids.
pub async fn referenced_user_ids(
    db: &impl ConnectionTrait,
    after: Uuid,
    limit: u64,
) -> Result<Vec<Uuid>, DbErr> {
    db.query_all(
        db.get_database_backend()
            .build(&referenced_user_ids_query(after, limit)),
    )
    .await?
    .into_iter()
    .map(|row| row.try_get_many_by_index::<(Uuid,)>().map(|(id,)| id))
    .collect()
}

fn referenced_user_ids_query(after: Uuid, limit: u64) -> SelectStatement {
    let user_id = Alias::new("user_id");
    let mut user_ids = select_column(
        challenges_user_subtasks::Entity,
        challenges_user_subtasks::Column::UserId,
    );
    for query in [
        select_column(challenges_ban::Entity, challenges_ban::Column::UserId),
        select_column(challenges_ban::Entity, challenges_ban::Column::Creator),
        select_column(
            challenges_subtask_reports::Entity,
            challenges_subtask_reports::Column::UserId,
        ),
        select_column(
            challenges_multiple_choice_attempts::Entity,
            challenges_multiple_choice_attempts::Column::UserId,
        ),
        select_column(
            challenges_question_attempts::Entity,
            challenges_question_attempts::Column::UserId,
        ),
        select_column(
            challenges_matching_attempts::Entity,
            challenges_matching_attempts::Column::UserId,
        ),
        select_column(
            challenges_coding_challenge_submissions::Entity,
            challenges_coding_challenge_submissions::Column::Creator,
        ),
        select_column(
            challenges_subtasks::Entity,
            challenges_subtasks::Column::Creator,
        ),
        select_column(challenges_tasks::Entity, challenges_tasks::Column::Creator),
    ] {
        user_ids.union(UnionType::Distinct, query);
    }

    Query::select()
        .column(user_id.clone())
        .from_subquery(user_ids, Alias::new("user_ids"))
        // Ids are traversed in ascending order starting at the nil uuid, which
        // is the placeholder for bans that have not been created by a user and
        // is therefore never returned. Null ids are skipped as well.
        .and_where(Expr::col(user_id.clone()).gt(after))
        .order_by(user_id, Order::Asc)
        .limit(limit)
        .to_owned()
}

fn select_column(table: impl IntoTableRef, column: impl IntoColumnRef) -> SelectStatement {
    Query::select().column(column).from(table).to_owned()
}

#[cfg(test)]
mod tests {
    use sea_orm::sea_query::PostgresQueryBuilder;

    use super::*;

    /// Every column in the challenges database that contains a user id.
    const USER_ID_COLUMNS: &[(&str, &str)] = &[
        ("challenges_user_subtasks", "user_id"),
        ("challenges_ban", "user_id"),
        ("challenges_ban", "creator"),
        ("challenges_subtask_reports", "user_id"),
        ("challenges_multiple_choice_attempts", "user_id"),
        ("challenges_question_attempts", "user_id"),
        ("challenges_matching_attempts", "user_id"),
        ("challenges_coding_challenge_submissions", "creator"),
        ("challenges_subtasks", "creator"),
        ("challenges_tasks", "creator"),
    ];

    #[test]
    fn test_referenced_user_ids_query_covers_all_columns() {
        let query = referenced_user_ids_query(Uuid::nil(), 500).to_string(PostgresQueryBuilder);
        for (table, column) in USER_ID_COLUMNS {
            assert!(
                query.contains(&format!(r#"SELECT "{column}" FROM "{table}""#)),
                "{table}.{column} is missing from {query}"
            );
        }
    }

    #[test]
    fn test_referenced_user_ids_query_skips_nil_uuid() {
        let query = referenced_user_ids_query(Uuid::nil(), 500).to_string(PostgresQueryBuilder);
        assert!(query.contains(r#""user_id" > '00000000-0000-0000-0000-000000000000'"#));
        assert!(query.contains(r#"ORDER BY "user_id" ASC LIMIT 500"#));
    }
}
