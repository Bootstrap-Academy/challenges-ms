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
                    .table(Matching::Table)
                    .col(ColumnDef::new(Matching::SubtaskId).uuid().primary_key())
                    .col(
                        ColumnDef::new(Matching::Left)
                            .array(ColumnType::Text)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Matching::Right)
                            .array(ColumnType::Text)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Matching::Solution)
                            .array(ColumnType::SmallInteger)
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Matching::Table, Matching::SubtaskId)
                            .to(Subtask::Table, Subtask::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(MatchingAttempt::Table)
                    .col(ColumnDef::new(MatchingAttempt::Id).uuid().primary_key())
                    .col(
                        ColumnDef::new(MatchingAttempt::MatchingId)
                            .uuid()
                            .not_null(),
                    )
                    .col(ColumnDef::new(MatchingAttempt::UserId).uuid().not_null())
                    .col(
                        ColumnDef::new(MatchingAttempt::Timestamp)
                            .timestamp()
                            .not_null(),
                    )
                    .col(ColumnDef::new(MatchingAttempt::Solved).boolean().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(MatchingAttempt::Table, MatchingAttempt::MatchingId)
                            .to(Matching::Table, Matching::SubtaskId)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(MatchingAttempt::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(Matching::Table).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(Iden)]
enum Matching {
    #[iden = "challenges_matchings"]
    Table,
    SubtaskId,
    Left,
    Right,
    Solution,
}

#[derive(Iden)]
enum MatchingAttempt {
    #[iden = "challenges_matching_attempts"]
    Table,
    Id,
    MatchingId,
    UserId,
    Timestamp,
    Solved,
}
