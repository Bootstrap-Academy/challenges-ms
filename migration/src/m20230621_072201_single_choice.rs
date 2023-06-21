use sea_orm_migration::prelude::*;

use crate::m20230322_163425_challenges_init::MultipleChoice;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(MultipleChoice::Table)
                    .add_column(
                        ColumnDef::new(MultipleChoice::SingleChoice)
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
                    .table(MultipleChoice::Table)
                    .drop_column(MultipleChoice::SingleChoice)
                    .to_owned(),
            )
            .await
    }
}
