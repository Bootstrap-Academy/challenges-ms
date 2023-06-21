use sea_orm_migration::prelude::*;

use crate::{
    m20230322_163425_challenges_init::Subtask, m20230619_084345_user_subtasks::UserSubtask,
    m20230620_093716_reports::Report,
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .table(Report::Table)
                    .name("challenges_subtask_reports_subtask_id_fkey")
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .from(Report::Table, Report::SubtaskId)
                    .to(Subtask::Table, Subtask::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .table(UserSubtask::Table)
                    .name("challenges_unlocked_subtasks_subtask_id_fkey")
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .from(UserSubtask::Table, UserSubtask::SubtaskId)
                    .to(Subtask::Table, Subtask::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .table(UserSubtask::Table)
                    .name("challenges_user_subtasks_subtask_id_fkey")
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("challenges_unlocked_subtasks_subtask_id_fkey")
                    .from(UserSubtask::Table, UserSubtask::SubtaskId)
                    .to(Subtask::Table, Subtask::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}
