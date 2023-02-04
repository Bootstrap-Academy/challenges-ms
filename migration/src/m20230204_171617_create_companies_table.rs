use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Company::Table)
                    .col(ColumnDef::new(Company::Id).uuid().primary_key())
                    .col(ColumnDef::new(Company::Name).text().not_null())
                    .col(ColumnDef::new(Company::Description).text())
                    .col(ColumnDef::new(Company::Website).text())
                    .col(ColumnDef::new(Company::YoutubeVideo).text())
                    .col(ColumnDef::new(Company::TwitterHandle).text())
                    .col(ColumnDef::new(Company::InstagramHandle).text())
                    .col(ColumnDef::new(Company::LogoUrl).text())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Company::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
pub enum Company {
    #[iden = "jobs_companies"]
    Table,
    Id,
    Name,
    Description,
    Website,
    YoutubeVideo,
    TwitterHandle,
    InstagramHandle,
    LogoUrl,
}
