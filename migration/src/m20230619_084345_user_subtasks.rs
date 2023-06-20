use sea_orm_migration::prelude::*;

use crate::m20230618_150706_add_unlocked_subtasks::UnlockedSubtask;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .rename_table(
                Table::rename()
                    .table(UnlockedSubtask::Table, UserSubtask::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(UserSubtask::Table)
                    .rename_column(UnlockedSubtask::Timestamp, UserSubtask::UnlockedTimestamp)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(UserSubtask::Table)
                    .modify_column(
                        ColumnDef::new(UserSubtask::UnlockedTimestamp)
                            .timestamp()
                            .null(),
                    )
                    .add_column(ColumnDef::new(UserSubtask::SolvedTimestamp).timestamp())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(UserSubtask::Table)
                    .and_where(Expr::col(UserSubtask::UnlockedTimestamp).is_null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(UserSubtask::Table)
                    .drop_column(UserSubtask::SolvedTimestamp)
                    .modify_column(
                        ColumnDef::new(UserSubtask::UnlockedTimestamp)
                            .timestamp()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(UserSubtask::Table)
                    .rename_column(UserSubtask::UnlockedTimestamp, UnlockedSubtask::Timestamp)
                    .to_owned(),
            )
            .await?;

        manager
            .rename_table(
                Table::rename()
                    .table(UserSubtask::Table, UnlockedSubtask::Table)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(Iden)]
pub enum UserSubtask {
    #[iden = "challenges_user_subtasks"]
    Table,
    UnlockedTimestamp,
    SolvedTimestamp,
    Rating,
    RatingTimestamp,
}
