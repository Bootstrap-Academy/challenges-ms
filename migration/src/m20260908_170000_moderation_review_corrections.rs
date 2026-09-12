use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(include_str!("moderation_review_corrections.sql"))
            .await?;
        manager
            .get_connection()
            .execute_unprepared(include_str!("moderation_review_challenges.sql"))
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Custom(
            "Review corrections retain case evidence; use a reviewed preservation migration".into(),
        ))
    }
}
