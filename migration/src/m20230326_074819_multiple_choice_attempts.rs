use sea_orm_migration::prelude::*;

use crate::m20230322_163425_challenges_init::MultipleChoice;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(MultipleChoiceAttempt::Table)
                    .col(
                        ColumnDef::new(MultipleChoiceAttempt::Id)
                            .uuid()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(MultipleChoiceAttempt::QuestionId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(MultipleChoiceAttempt::UserId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(MultipleChoiceAttempt::Timestamp)
                            .timestamp()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(MultipleChoiceAttempt::Solved)
                            .boolean()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(
                                MultipleChoiceAttempt::Table,
                                MultipleChoiceAttempt::QuestionId,
                            )
                            .to(MultipleChoice::Table, MultipleChoice::SubtaskId)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(MultipleChoiceAttempt::Table).to_owned())
            .await
    }
}

#[derive(Iden, Clone, Copy)]
pub enum MultipleChoiceAttempt {
    #[iden = "challenges_multiple_choice_attempts"]
    Table,
    Id,
    QuestionId,
    UserId,
    Timestamp,
    Solved,
}
