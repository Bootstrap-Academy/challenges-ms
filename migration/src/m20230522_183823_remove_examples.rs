use sea_orm_migration::prelude::*;

use crate::m20230322_163425_challenges_init::{CodingChallenge, CodingChallengeExample};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(CodingChallengeExample::Table)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CodingChallengeExample::Table)
                    .col(
                        ColumnDef::new(CodingChallengeExample::Id)
                            .uuid()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(CodingChallengeExample::ChallengeId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CodingChallengeExample::Input)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CodingChallengeExample::Output)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(CodingChallengeExample::Explanation).text())
                    .foreign_key(
                        ForeignKey::create()
                            .from(
                                CodingChallengeExample::Table,
                                CodingChallengeExample::ChallengeId,
                            )
                            .to(CodingChallenge::Table, CodingChallenge::SubtaskId)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
