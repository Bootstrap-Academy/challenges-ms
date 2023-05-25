use sea_orm_migration::prelude::*;

use crate::m20230322_163425_challenges_init::CodingChallenge;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(CodingChallenge::Table)
                    .add_column(
                        ColumnDef::new(CodingChallenge::StaticTests)
                            .integer()
                            .default(10)
                            .not_null(),
                    )
                    .add_column(
                        ColumnDef::new(CodingChallenge::RandomTests)
                            .integer()
                            .default(10)
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(CodingChallenge::Table)
                    .drop_column(CodingChallenge::StaticTests)
                    .drop_column(CodingChallenge::RandomTests)
                    .to_owned(),
            )
            .await
    }
}
