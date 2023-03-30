use sea_orm_migration::prelude::*;

use crate::m20230322_163425_challenges_init::Challenge;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Challenge::Table)
                    .add_column(
                        ColumnDef::new(Challenge::SkillIds)
                            .array(ColumnType::Text)
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
                    .table(Challenge::Table)
                    .drop_column(Challenge::SkillIds)
                    .to_owned(),
            )
            .await
    }
}
