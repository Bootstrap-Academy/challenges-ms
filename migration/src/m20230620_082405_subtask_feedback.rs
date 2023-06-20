use sea_orm_migration::{prelude::*, sea_query::extension::postgres::Type};

use crate::m20230619_084345_user_subtasks::UserSubtask;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_type(
                Type::create()
                    .as_enum(Rating::Type)
                    .values([Rating::Positive, Rating::Neutral, Rating::Negative])
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(UserSubtask::Table)
                    .add_column(ColumnDef::new(UserSubtask::Rating).custom(Rating::Type))
                    .add_column(ColumnDef::new(UserSubtask::RatingTimestamp).timestamp())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(UserSubtask::Table)
                    .drop_column(UserSubtask::Rating)
                    .drop_column(UserSubtask::RatingTimestamp)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_type(Type::drop().name(Rating::Type).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(Iden)]
enum Rating {
    #[iden = "challenges_rating"]
    Type,
    Positive,
    Neutral,
    Negative,
}
