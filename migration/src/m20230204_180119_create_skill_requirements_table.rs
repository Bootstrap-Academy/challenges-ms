use sea_orm_migration::prelude::*;

use crate::m20230204_171643_create_jobs_table::Job;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(SkillRequirement::Table)
                    .col(ColumnDef::new(SkillRequirement::JobId).uuid())
                    .col(ColumnDef::new(SkillRequirement::SkillId).uuid())
                    .col(ColumnDef::new(SkillRequirement::Level).integer().not_null())
                    .primary_key(
                        Index::create()
                            .col(SkillRequirement::JobId)
                            .col(SkillRequirement::SkillId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(SkillRequirement::Table, SkillRequirement::JobId)
                            .to(Job::Table, Job::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SkillRequirement::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum SkillRequirement {
    #[iden = "jobs_skill_requirements"]
    Table,
    JobId,
    SkillId,
    Level,
}
