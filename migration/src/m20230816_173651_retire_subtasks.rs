use sea_orm_migration::prelude::*;

use crate::m20230322_163425_challenges_init::Subtask;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Subtask::Table)
                    .add_column(
                        ColumnDef::new(Subtask::Retired)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Subtask::Table)
                    .drop_column(Subtask::Retired)
                    .to_owned(),
            )
            .await
    }
}
