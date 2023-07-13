use sea_orm_migration::prelude::*;

use crate::m20230620_093716_reports::Report;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(Report::Table)
                    .and_where(Expr::col(Report::CompletedTimestamp).is_not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Report::Table)
                    .drop_column(Report::CompletedBy)
                    .drop_column(Report::CompletedTimestamp)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Report::Table)
                    .add_column(ColumnDef::new(Report::CompletedBy).uuid())
                    .add_column(ColumnDef::new(Report::CompletedTimestamp).timestamp())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}
