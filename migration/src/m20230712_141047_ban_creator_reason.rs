use sea_orm_migration::{prelude::*, sea_orm::prelude::Uuid};

use crate::m20230620_163944_ban::Ban;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Ban::Table)
                    .add_column(
                        ColumnDef::new(Ban::Creator)
                            .uuid()
                            .not_null()
                            .default(Uuid::nil()),
                    )
                    .add_column(ColumnDef::new(Ban::Reason).text().not_null().default(""))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Ban::Table)
                    .drop_column(Ban::Creator)
                    .drop_column(Ban::Reason)
                    .to_owned(),
            )
            .await
    }
}
