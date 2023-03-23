use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ChallengeCategory::Table)
                    .col(ColumnDef::new(ChallengeCategory::Id).uuid().primary_key())
                    .col(ColumnDef::new(ChallengeCategory::Title).text().not_null())
                    .col(
                        ColumnDef::new(ChallengeCategory::Description)
                            .text()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Task::Table)
                    .col(ColumnDef::new(Task::Id).uuid().primary_key())
                    .col(ColumnDef::new(Task::Title).text().not_null())
                    .col(ColumnDef::new(Task::Description).text().not_null())
                    .col(ColumnDef::new(Task::Creator).uuid().not_null())
                    .col(
                        ColumnDef::new(Task::CreationTimestamp)
                            .timestamp()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Challenge::Table)
                    .col(ColumnDef::new(Challenge::TaskId).uuid().primary_key())
                    .col(ColumnDef::new(Challenge::CategoryId).uuid().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(Challenge::Table, Challenge::TaskId)
                            .to(Task::Table, Task::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Challenge::Table, Challenge::CategoryId)
                            .to(ChallengeCategory::Table, ChallengeCategory::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(SkillTask::Table)
                    .col(ColumnDef::new(SkillTask::TaskId).uuid().primary_key())
                    .col(ColumnDef::new(SkillTask::SkillId).uuid().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(SkillTask::Table, SkillTask::TaskId)
                            .to(Task::Table, Task::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Subtask::Table)
                    .col(ColumnDef::new(Subtask::Id).uuid().primary_key())
                    .col(ColumnDef::new(Subtask::TaskId).uuid().not_null())
                    .col(ColumnDef::new(Subtask::Creator).uuid().not_null())
                    .col(
                        ColumnDef::new(Subtask::CreationTimestamp)
                            .timestamp()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Subtask::Xp).big_integer().not_null())
                    .col(ColumnDef::new(Subtask::Coins).big_integer().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(Subtask::Table, Subtask::TaskId)
                            .to(Task::Table, Task::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(MultipleChoice::Table)
                    .col(
                        ColumnDef::new(MultipleChoice::SubtaskId)
                            .uuid()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(MultipleChoice::Question).text().not_null())
                    .col(
                        ColumnDef::new(MultipleChoice::Answers)
                            .array(ColumnType::Text)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(MultipleChoice::CorrectAnswers)
                            .big_integer()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(MultipleChoice::Table, MultipleChoice::SubtaskId)
                            .to(Subtask::Table, Subtask::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(CodingChallenge::Table)
                    .col(
                        ColumnDef::new(CodingChallenge::SubtaskId)
                            .uuid()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(CodingChallenge::TimeLimit)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CodingChallenge::MemoryLimit)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(CodingChallenge::Evaluator).text().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(CodingChallenge::Table, CodingChallenge::SubtaskId)
                            .to(Subtask::Table, Subtask::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(CodingChallengeExample::Table)
                    .col(
                        ColumnDef::new(CodingChallengeExample::Id)
                            .uuid()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(CodingChallengeExample::ChallengeId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CodingChallengeExample::Input)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CodingChallengeExample::Output)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(CodingChallengeExample::Explanation).text())
                    .foreign_key(
                        ForeignKey::create()
                            .from(
                                CodingChallengeExample::Table,
                                CodingChallengeExample::ChallengeId,
                            )
                            .to(CodingChallenge::Table, CodingChallenge::SubtaskId)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(CodingChallengeExample::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(CodingChallenge::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(MultipleChoice::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Subtask::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(SkillTask::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Challenge::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Task::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(ChallengeCategory::Table).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(Iden)]
enum ChallengeCategory {
    #[iden = "challenges_challenge_categories"]
    Table,
    Id,
    Title,
    Description,
}

#[derive(Iden)]
enum Challenge {
    #[iden = "challenges_challenges"]
    Table,
    CategoryId,
    TaskId,
}

#[derive(Iden)]
enum SkillTask {
    #[iden = "challenges_skill_tasks"]
    Table,
    SkillId,
    TaskId,
}

#[derive(Iden)]
enum Task {
    #[iden = "challenges_tasks"]
    Table,
    Id,
    Title,
    Description,
    Creator,
    CreationTimestamp,
}

#[derive(Iden)]
enum Subtask {
    #[iden = "challenges_subtasks"]
    Table,
    Id,
    TaskId,
    Creator,
    CreationTimestamp,
    Xp,
    Coins,
}

#[derive(Iden)]
enum MultipleChoice {
    #[iden = "challenges_multiple_choice_quizes"]
    Table,
    SubtaskId,
    Question,
    Answers,
    CorrectAnswers,
}

#[derive(Iden)]
enum CodingChallenge {
    #[iden = "challenges_coding_challenges"]
    Table,
    SubtaskId,
    TimeLimit,
    MemoryLimit,
    Evaluator,
}

#[derive(Iden)]
enum CodingChallengeExample {
    #[iden = "challenges_coding_challenge_example"]
    Table,
    Id,
    ChallengeId,
    Input,
    Output,
    Explanation,
}
