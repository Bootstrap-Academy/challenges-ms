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
                    .table(Question::Table)
                    .col(ColumnDef::new(Question::SubtaskId).uuid().primary_key())
                    .col(ColumnDef::new(Question::Question).text().not_null())
                    .col(
                        ColumnDef::new(Question::Answers)
                            .array(ColumnType::Text)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Question::CaseSensitive).boolean().not_null())
                    .col(ColumnDef::new(Question::AsciiLetters).boolean().not_null())
                    .col(ColumnDef::new(Question::Digits).boolean().not_null())
                    .col(ColumnDef::new(Question::Punctuation).boolean().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(Question::Table, Question::SubtaskId)
                            .to(Subtask::Table, Subtask::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(QuestionAttempt::Table)
                    .col(ColumnDef::new(QuestionAttempt::Id).uuid().primary_key())
                    .col(
                        ColumnDef::new(QuestionAttempt::QuestionId)
                            .uuid()
                            .not_null(),
                    )
                    .col(ColumnDef::new(QuestionAttempt::UserId).uuid().not_null())
                    .col(
                        ColumnDef::new(QuestionAttempt::Timestamp)
                            .timestamp()
                            .not_null(),
                    )
                    .col(ColumnDef::new(QuestionAttempt::Solved).boolean().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(QuestionAttempt::Table, QuestionAttempt::QuestionId)
                            .to(Question::Table, Question::SubtaskId)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(QuestionAttempt::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(Question::Table).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(Iden)]
enum Question {
    #[iden = "challenges_questions"]
    Table,
    SubtaskId,
    Question,
    Answers,
    CaseSensitive,
    AsciiLetters,
    Digits,
    Punctuation,
}

#[derive(Iden)]
enum QuestionAttempt {
    #[iden = "challenges_question_attempts"]
    Table,
    Id,
    QuestionId,
    UserId,
    Timestamp,
    Solved,
}
