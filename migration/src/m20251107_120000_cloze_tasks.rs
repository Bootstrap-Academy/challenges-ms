use sea_orm_migration::{prelude::*, sea_orm::Statement};

use crate::m20230322_163425_challenges_init::Subtask;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute(Statement::from_string(
                manager.get_database_backend(),
                "ALTER TYPE challenges_subtask_type ADD VALUE IF NOT EXISTS 'cloze'",
            ))
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Cloze::Table)
                    .col(
                        ColumnDef::new(Cloze::SubtaskId)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Cloze::Content).text().not_null())
                    .col(
                        ColumnDef::new(Cloze::CaseSensitive)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Cloze::Table, Cloze::SubtaskId)
                            .to(Subtask::Table, Subtask::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(ClozeOption::Table)
                    .col(
                        ColumnDef::new(ClozeOption::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ClozeOption::ClozeId).uuid().not_null())
                    .col(ColumnDef::new(ClozeOption::Position).integer().not_null())
                    .col(ColumnDef::new(ClozeOption::Label).text().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(ClozeOption::Table, ClozeOption::ClozeId)
                            .to(Cloze::Table, Cloze::SubtaskId)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(ClozeBlank::Table)
                    .col(
                        ColumnDef::new(ClozeBlank::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ClozeBlank::ClozeId).uuid().not_null())
                    .col(ColumnDef::new(ClozeBlank::Placeholder).integer().not_null())
                    .col(ColumnDef::new(ClozeBlank::Answer).text().not_null())
                    .col(
                        ColumnDef::new(ClozeBlank::Synonyms)
                            .array(ColumnType::Text)
                            .not_null(),
                    )
                    .col(ColumnDef::new(ClozeBlank::CorrectOptionId).uuid().null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(ClozeBlank::Table, ClozeBlank::ClozeId)
                            .to(Cloze::Table, Cloze::SubtaskId)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(ClozeBlank::Table, ClozeBlank::CorrectOptionId)
                            .to(ClozeOption::Table, ClozeOption::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(ClozeAttempt::Table)
                    .col(
                        ColumnDef::new(ClozeAttempt::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ClozeAttempt::ClozeId).uuid().not_null())
                    .col(ColumnDef::new(ClozeAttempt::UserId).uuid().not_null())
                    .col(
                        ColumnDef::new(ClozeAttempt::Timestamp)
                            .timestamp()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ClozeAttempt::Correct).integer().not_null())
                    .col(ColumnDef::new(ClozeAttempt::Total).integer().not_null())
                    .col(ColumnDef::new(ClozeAttempt::Solved).boolean().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(ClozeAttempt::Table, ClozeAttempt::ClozeId)
                            .to(Cloze::Table, Cloze::SubtaskId)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_cloze_blanks_unique_placeholder")
                    .table(ClozeBlank::Table)
                    .col(ClozeBlank::ClozeId)
                    .col(ClozeBlank::Placeholder)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_cloze_blanks_unique_option")
                    .table(ClozeBlank::Table)
                    .col(ClozeBlank::CorrectOptionId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_cloze_options_position")
                    .table(ClozeOption::Table)
                    .col(ClozeOption::ClozeId)
                    .col(ClozeOption::Position)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_cloze_options_position")
                    .table(ClozeOption::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_cloze_blanks_unique_option")
                    .table(ClozeBlank::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_cloze_blanks_unique_placeholder")
                    .table(ClozeBlank::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(ClozeAttempt::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(ClozeBlank::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(ClozeOption::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(Cloze::Table).to_owned())
            .await?;

        // The enum value cannot easily be removed once added, so the down migration
        // intentionally leaves it in place.
        Ok(())
    }
}

#[derive(Iden)]
pub enum Cloze {
    #[iden = "challenges_clozes"]
    Table,
    SubtaskId,
    Content,
    CaseSensitive,
}

#[derive(Iden)]
pub enum ClozeOption {
    #[iden = "challenges_cloze_options"]
    Table,
    Id,
    ClozeId,
    Position,
    Label,
}

#[derive(Iden)]
pub enum ClozeBlank {
    #[iden = "challenges_cloze_blanks"]
    Table,
    Id,
    ClozeId,
    Placeholder,
    Answer,
    Synonyms,
    CorrectOptionId,
}

#[derive(Iden)]
pub enum ClozeAttempt {
    #[iden = "challenges_cloze_attempts"]
    Table,
    Id,
    ClozeId,
    UserId,
    Timestamp,
    Correct,
    Total,
    Solved,
}
