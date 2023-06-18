use sea_orm_migration::prelude::*;

use crate::m20230322_163425_challenges_init::Subtask;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(UnlockedSubtask::Table)
                    .col(ColumnDef::new(UnlockedSubtask::UserId).uuid().not_null())
                    .col(ColumnDef::new(UnlockedSubtask::SubtaskId).uuid().not_null())
                    .col(
                        ColumnDef::new(UnlockedSubtask::Timestamp)
                            .timestamp()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(UnlockedSubtask::UserId)
                            .col(UnlockedSubtask::SubtaskId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(UnlockedSubtask::Table, UnlockedSubtask::SubtaskId)
                            .to(Subtask::Table, Subtask::Id),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(UnlockedSubtask::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum UnlockedSubtask {
    #[iden = "challenges_unlocked_subtasks"]
    Table,
    UserId,
    SubtaskId,
    Timestamp,
}
