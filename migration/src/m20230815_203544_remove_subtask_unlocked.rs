use sea_orm_migration::prelude::*;

use crate::m20230619_084345_user_subtasks::UserSubtask;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(UserSubtask::Table)
                    .drop_column(UserSubtask::UnlockedTimestamp)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(UserSubtask::Table)
                    .add_column(
                        ColumnDef::new(UserSubtask::UnlockedTimestamp)
                            .date_time()
                            .null(),
                    )
                    .to_owned(),
            )
            .await
    }
}
