use std::collections::{HashMap, HashSet};

use sea_orm_migration::{
    prelude::*,
    sea_orm::prelude::{DateTime, Uuid},
};

use crate::{
    m20230326_074819_multiple_choice_attempts::MultipleChoiceAttempt,
    m20230527_151432_cc_submissions::Submission, m20230619_084345_user_subtasks::UserSubtask,
    m20230621_074711_questions::QuestionAttempt, m20230621_141228_matchings::MatchingAttempt,
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(UserSubtask::Table)
                    .add_column(
                        ColumnDef::new(UserSubtask::LastAttemptTimestamp)
                            .null()
                            .timestamp(),
                    )
                    .add_column(
                        ColumnDef::new(UserSubtask::Attempts)
                            .not_null()
                            .integer()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await?;

        async fn get_user_subtasks<C: IntoColumnRef + Copy + 'static>(
            manager: &SchemaManager<'_>,
            table: impl Iden + Copy + 'static,
            subtask_id: C,
            user_id: C,
            timestamp: impl Iden + 'static,
        ) -> Result<Vec<((Uuid, Uuid), (DateTime, i64))>, DbErr> {
            let conn = manager.get_connection();
            let builder = conn.get_database_backend();
            Ok(conn
                .query_all(
                    builder.build(
                        Query::select()
                            .from(table)
                            .column(subtask_id)
                            .column(user_id)
                            .expr(Expr::col((table, timestamp)).max())
                            .expr(Expr::col(Asterisk).count())
                            .group_by_columns([subtask_id, user_id]),
                    ),
                )
                .await?
                .into_iter()
                .map(|x| {
                    (
                        (
                            x.try_get_by_index::<Uuid>(0).unwrap(),
                            x.try_get_by_index::<Uuid>(1).unwrap(),
                        ),
                        (
                            x.try_get_by_index::<DateTime>(2).unwrap(),
                            x.try_get_by_index::<i64>(3).unwrap(),
                        ),
                    )
                })
                .collect::<Vec<_>>())
        }

        let user_attempts = std::iter::empty()
            .chain(
                get_user_subtasks(
                    manager,
                    MultipleChoiceAttempt::Table,
                    MultipleChoiceAttempt::QuestionId,
                    MultipleChoiceAttempt::UserId,
                    MultipleChoiceAttempt::Timestamp,
                )
                .await?,
            )
            .chain(
                get_user_subtasks(
                    manager,
                    QuestionAttempt::Table,
                    QuestionAttempt::QuestionId,
                    QuestionAttempt::UserId,
                    QuestionAttempt::Timestamp,
                )
                .await?,
            )
            .chain(
                get_user_subtasks(
                    manager,
                    MatchingAttempt::Table,
                    MatchingAttempt::MatchingId,
                    MatchingAttempt::UserId,
                    MatchingAttempt::Timestamp,
                )
                .await?,
            )
            .chain(
                get_user_subtasks(
                    manager,
                    Submission::Table,
                    Submission::SubtaskId,
                    Submission::Creator,
                    Submission::CreationTimestamp,
                )
                .await?,
            )
            .collect::<HashMap<_, _>>();

        let conn = manager.get_connection();
        let builder = conn.get_database_backend();
        let user_subtasks = conn
            .query_all(
                builder.build(
                    Query::select()
                        .from(UserSubtask::Table)
                        .column(UserSubtask::SubtaskId)
                        .column(UserSubtask::UserId),
                ),
            )
            .await?
            .into_iter()
            .map(|x| {
                (
                    x.try_get_by_index::<Uuid>(0).unwrap(),
                    x.try_get_by_index::<Uuid>(1).unwrap(),
                )
            })
            .collect::<HashSet<_>>();

        for ((subtask_id, user_id), (last_attempt, attempts)) in user_attempts {
            if user_subtasks.contains(&(subtask_id, user_id)) {
                manager
                    .exec_stmt(
                        Query::update()
                            .table(UserSubtask::Table)
                            .value(UserSubtask::LastAttemptTimestamp, last_attempt)
                            .value(UserSubtask::Attempts, attempts)
                            .and_where(Expr::col(UserSubtask::SubtaskId).eq(subtask_id))
                            .and_where(Expr::col(UserSubtask::UserId).eq(user_id))
                            .to_owned(),
                    )
                    .await?;
            } else {
                manager
                    .exec_stmt(
                        Query::insert()
                            .into_table(UserSubtask::Table)
                            .columns([
                                UserSubtask::SubtaskId,
                                UserSubtask::UserId,
                                UserSubtask::LastAttemptTimestamp,
                                UserSubtask::Attempts,
                            ])
                            .values_panic([
                                subtask_id.into(),
                                user_id.into(),
                                last_attempt.into(),
                                attempts.into(),
                            ])
                            .to_owned(),
                    )
                    .await?;
            }
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(UserSubtask::Table)
                    .drop_column(UserSubtask::LastAttemptTimestamp)
                    .drop_column(UserSubtask::Attempts)
                    .to_owned(),
            )
            .await
    }
}
