use sea_orm_migration::{prelude::*, sea_query::extension::postgres::Type};

use crate::m20230322_163425_challenges_init::CodingChallenge;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_type(
                Type::create()
                    .as_enum(Verdict::Type)
                    .values([
                        Verdict::Ok,
                        Verdict::WrongAnswer,
                        Verdict::InvalidOutputFormat,
                        Verdict::TimeLimitExceeded,
                        Verdict::MemoryLimitExceeded,
                        Verdict::NoOutput,
                        Verdict::CompilationError,
                        Verdict::RuntimeError,
                    ])
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Submission::Table)
                    .col(ColumnDef::new(Submission::Id).uuid().primary_key())
                    .col(ColumnDef::new(Submission::SubtaskId).uuid().not_null())
                    .col(ColumnDef::new(Submission::Creator).uuid().not_null())
                    .col(
                        ColumnDef::new(Submission::CreationTimestamp)
                            .timestamp()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Submission::Environment).text().not_null())
                    .col(ColumnDef::new(Submission::Code).text().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(Submission::Table, Submission::SubtaskId)
                            .to(CodingChallenge::Table, CodingChallenge::SubtaskId)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(SubmissionResult::Table)
                    .col(
                        ColumnDef::new(SubmissionResult::SubmissionId)
                            .uuid()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(SubmissionResult::Verdict)
                            .custom(Verdict::Type)
                            .not_null(),
                    )
                    .col(ColumnDef::new(SubmissionResult::Reason).text())
                    .col(ColumnDef::new(SubmissionResult::BuildStatus).integer())
                    .col(ColumnDef::new(SubmissionResult::BuildStderr).text())
                    .col(ColumnDef::new(SubmissionResult::BuildTime).integer())
                    .col(ColumnDef::new(SubmissionResult::BuildMemory).integer())
                    .col(ColumnDef::new(SubmissionResult::RunStatus).integer())
                    .col(ColumnDef::new(SubmissionResult::RunStderr).text())
                    .col(ColumnDef::new(SubmissionResult::RunTime).integer())
                    .col(ColumnDef::new(SubmissionResult::RunMemory).integer())
                    .foreign_key(
                        ForeignKey::create()
                            .from(SubmissionResult::Table, SubmissionResult::SubmissionId)
                            .to(Submission::Table, Submission::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SubmissionResult::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(Submission::Table).to_owned())
            .await?;

        manager
            .drop_type(Type::drop().name(Verdict::Type).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(Iden)]
enum Submission {
    #[iden = "challenges_coding_challenge_submissions"]
    Table,
    Id,
    SubtaskId,
    Creator,
    CreationTimestamp,
    Environment,
    Code,
}

#[derive(Iden)]
enum SubmissionResult {
    #[iden = "challenges_coding_challenge_result"]
    Table,
    SubmissionId,
    Verdict,
    Reason,
    BuildStatus,
    BuildStderr,
    BuildTime,
    BuildMemory,
    RunStatus,
    RunStderr,
    RunTime,
    RunMemory,
}

#[derive(Iden)]
pub enum Verdict {
    #[iden = "challenges_verdict"]
    Type,
    Ok,
    WrongAnswer,
    InvalidOutputFormat,
    TimeLimitExceeded,
    MemoryLimitExceeded,
    NoOutput,
    CompilationError,
    RuntimeError,
    PreCheckFailed,
}
