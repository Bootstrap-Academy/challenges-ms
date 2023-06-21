use sea_orm_migration::prelude::*;

use crate::m20230621_074711_questions::Question;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Question::Table)
                    .add_column(ColumnDef::new(Question::Blocks).array(ColumnType::Text))
                    .to_owned(),
            )
            .await?;

        manager
            .exec_stmt(
                Query::update()
                    .table(Question::Table)
                    .value(Question::Blocks, Vec::<String>::new())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Question::Table)
                    .modify_column(ColumnDef::new(Question::Blocks).not_null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Question::Table)
                    .drop_column(Question::Blocks)
                    .to_owned(),
            )
            .await
    }
}
