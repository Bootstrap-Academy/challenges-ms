use sea_orm_migration::{prelude::*, sea_query::extension::postgres::Type};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_type(
                Type::create()
                    .as_enum(BanAction::Type)
                    .values([BanAction::Create, BanAction::Report])
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Ban::Table)
                    .col(ColumnDef::new(Ban::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Ban::UserId).uuid().not_null())
                    .col(ColumnDef::new(Ban::Start).timestamp().not_null())
                    .col(ColumnDef::new(Ban::End).timestamp())
                    .col(
                        ColumnDef::new(Ban::Action)
                            .custom(BanAction::Type)
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Ban::Table).to_owned())
            .await?;

        manager
            .drop_type(Type::drop().name(BanAction::Type).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(Iden)]
enum Ban {
    #[iden = "challenges_ban"]
    Table,
    Id,
    UserId,
    Start,
    End,
    Action,
}

#[derive(Iden)]
enum BanAction {
    #[iden = "challenges_ban_action"]
    Type,
    Create,
    Report,
}
