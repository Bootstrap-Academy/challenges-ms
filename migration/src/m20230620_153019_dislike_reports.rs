use sea_orm_migration::{prelude::*, sea_query::extension::postgres::Type};

use crate::m20230620_093716_reports::{Report, ReportReason};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let sql = Type::alter()
            .name(ReportReason::Type)
            .add_value(ReportReason::Dislike)
            .to_string(PostgresQueryBuilder)
            .replace("ADD VALUE", "ADD VALUE IF NOT EXISTS");
        manager.get_connection().execute_unprepared(&sql).await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Report::Table)
                    .modify_column(ColumnDef::new(Report::UserId).null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(Report::Table)
                    .and_where(Expr::col(Report::UserId).is_null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Report::Table)
                    .modify_column(ColumnDef::new(Report::UserId).not_null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}
