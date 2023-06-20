use sea_orm_migration::{prelude::*, sea_query::extension::postgres::Type};

use crate::m20230322_163425_challenges_init::Subtask;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_type(
                Type::create()
                    .as_enum(ReportReason::Type)
                    .values([
                        ReportReason::Wrong,
                        ReportReason::Abuse,
                        ReportReason::UnrelatedSkill,
                        ReportReason::Other,
                    ])
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Report::Table)
                    .col(ColumnDef::new(Report::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Report::SubtaskId).uuid().not_null())
                    .col(ColumnDef::new(Report::UserId).uuid().not_null())
                    .col(ColumnDef::new(Report::Timestamp).timestamp().not_null())
                    .col(
                        ColumnDef::new(Report::Reason)
                            .custom(ReportReason::Type)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Report::Comment).text().not_null())
                    .col(ColumnDef::new(Report::CompletedBy).uuid())
                    .col(ColumnDef::new(Report::CompletedTimestamp).timestamp())
                    .foreign_key(
                        ForeignKey::create()
                            .from(Report::Table, Report::SubtaskId)
                            .to(Subtask::Table, Subtask::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Subtask::Table)
                    .add_column(
                        ColumnDef::new(Subtask::Enabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Subtask::Table)
                    .drop_column(Subtask::Enabled)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(Report::Table).to_owned())
            .await?;

        manager
            .drop_type(Type::drop().name(ReportReason::Type).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(Iden)]
enum Report {
    #[iden = "challenges_subtask_reports"]
    Table,
    Id,
    SubtaskId,
    UserId,
    Timestamp,
    Reason,
    Comment,
    CompletedBy,
    CompletedTimestamp,
}

#[derive(Iden)]
enum ReportReason {
    #[iden = "challenges_report_reason"]
    Type,
    Wrong,
    Abuse,
    UnrelatedSkill,
    Other,
}
