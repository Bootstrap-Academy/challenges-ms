use sea_orm_migration::prelude::*;

use crate::m20230322_163425_challenges_init::ChallengeCategory;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ChallengeCategory::Table)
                    .add_column(
                        ColumnDef::new(ChallengeCategory::CreationTimestamp)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ChallengeCategory::Table)
                    .drop_column(ChallengeCategory::CreationTimestamp)
                    .to_owned(),
            )
            .await
    }
}
