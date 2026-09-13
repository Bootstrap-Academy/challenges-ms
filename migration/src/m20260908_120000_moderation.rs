use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(include_str!("moderation.sql"))
            .await?;
        manager
            .get_connection()
            .execute_unprepared(include_str!("moderation_challenges.sql"))
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Custom("Moderation migration retains restriction/appeal evidence; use a reviewed preservation migration".into()))
    }
}
