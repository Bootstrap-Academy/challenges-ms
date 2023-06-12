use sea_orm_migration::prelude::*;

use crate::m20230322_163425_challenges_init::{Challenge, CourseTask, Task};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(CourseTask::Table)
                    .modify_column(ColumnDef::new(CourseTask::SectionId).text().null())
                    .modify_column(ColumnDef::new(CourseTask::LectureId).text().null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Task::Table)
                    .drop_column(Task::Title)
                    .drop_column(Task::Description)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Challenge::Table)
                    .add_column(
                        ColumnDef::new(Challenge::Title)
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .add_column(
                        ColumnDef::new(Challenge::Description)
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(CourseTask::Table)
                    .modify_column(ColumnDef::new(CourseTask::SectionId).text().not_null())
                    .modify_column(ColumnDef::new(CourseTask::LectureId).text().not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Task::Table)
                    .add_column(ColumnDef::new(Task::Title).text().not_null().default(""))
                    .add_column(
                        ColumnDef::new(Task::Description)
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Challenge::Table)
                    .drop_column(Challenge::Title)
                    .drop_column(Challenge::Description)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}
