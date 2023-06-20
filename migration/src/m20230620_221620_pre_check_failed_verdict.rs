use sea_orm_migration::{prelude::*, sea_query::extension::postgres::Type};

use crate::m20230527_151432_cc_submissions::Verdict;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let sql = Type::alter()
            .name(Verdict::Type)
            .add_value(Verdict::PreCheckFailed)
            .to_string(PostgresQueryBuilder)
            .replace("ADD VALUE", "ADD VALUE IF NOT EXISTS");
        manager.get_connection().execute_unprepared(&sql).await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
