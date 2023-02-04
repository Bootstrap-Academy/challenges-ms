use sea_orm_migration::{prelude::*, sea_query::extension::postgres::Type};

use crate::m20230204_171617_create_companies_table::Company;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_type(
                Type::create()
                    .as_enum(JobType::Type)
                    .values([
                        JobType::FullTime,
                        JobType::PartTime,
                        JobType::Internship,
                        JobType::Temporary,
                        JobType::MiniJob,
                    ])
                    .to_owned(),
            )
            .await?;

        manager
            .create_type(
                Type::create()
                    .as_enum(ProfessionalLevel::Type)
                    .values([
                        ProfessionalLevel::Entry,
                        ProfessionalLevel::Junior,
                        ProfessionalLevel::Senior,
                        ProfessionalLevel::Manager,
                    ])
                    .to_owned(),
            )
            .await?;

        manager
            .create_type(
                Type::create()
                    .as_enum(SalaryUnit::Type)
                    .values([SalaryUnit::Euro, SalaryUnit::Morphcoins])
                    .to_owned(),
            )
            .await?;

        manager
            .create_type(
                Type::create()
                    .as_enum(SalaryPer::Type)
                    .values([
                        SalaryPer::Once,
                        SalaryPer::Task,
                        SalaryPer::Hour,
                        SalaryPer::Day,
                        SalaryPer::Month,
                        SalaryPer::Year,
                    ])
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Job::Table)
                    .col(ColumnDef::new(Job::Id).uuid().primary_key())
                    .col(ColumnDef::new(Job::CompanyId).uuid().not_null())
                    .col(ColumnDef::new(Job::Title).text().not_null())
                    .col(ColumnDef::new(Job::Description).text().not_null())
                    .col(ColumnDef::new(Job::Location).text().not_null())
                    .col(ColumnDef::new(Job::Remote).boolean().not_null())
                    .col(ColumnDef::new(Job::Type).custom(JobType::Type).not_null())
                    .col(
                        ColumnDef::new(Job::Responsibilities)
                            .array(ColumnType::Text)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Job::ProfessionalLevel)
                            .custom(ProfessionalLevel::Type)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Job::SalaryMin).integer().not_null())
                    .col(ColumnDef::new(Job::SalaryMax).integer().not_null())
                    .col(
                        ColumnDef::new(Job::SalaryUnit)
                            .custom(SalaryUnit::Type)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Job::SalaryPer)
                            .custom(SalaryPer::Type)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Job::Contact).text().not_null())
                    .col(ColumnDef::new(Job::LastUpdate).date_time().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(Job::Table, Job::CompanyId)
                            .to(Company::Table, Company::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Job::Table).to_owned())
            .await?;
        manager
            .drop_type(Type::drop().name(JobType::Type).to_owned())
            .await?;
        manager
            .drop_type(Type::drop().name(ProfessionalLevel::Type).to_owned())
            .await?;
        manager
            .drop_type(Type::drop().name(SalaryUnit::Type).to_owned())
            .await?;
        manager
            .drop_type(Type::drop().name(SalaryPer::Type).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(Iden)]
pub enum JobType {
    #[iden = "jobs_job_type"]
    Type,
    FullTime,
    PartTime,
    Internship,
    Temporary,
    MiniJob,
}

#[derive(Iden)]
pub enum ProfessionalLevel {
    #[iden = "jobs_professional_level"]
    Type,
    Entry,
    Junior,
    Senior,
    Manager,
}

#[derive(Iden)]
pub enum SalaryUnit {
    #[iden = "jobs_salary_unit"]
    Type,
    Euro,
    Morphcoins,
}

#[derive(Iden)]
pub enum SalaryPer {
    #[iden = "jobs_salary_per"]
    Type,
    Once,
    Task,
    Hour,
    Day,
    Month,
    Year,
}

#[derive(Iden)]
pub enum Job {
    #[iden = "jobs_jobs"]
    Table,
    Id,
    CompanyId,
    Title,
    Description,
    Location,
    Remote,
    Type,
    Responsibilities,
    ProfessionalLevel,
    SalaryMin,
    SalaryMax,
    SalaryUnit,
    SalaryPer,
    Contact,
    LastUpdate,
}
