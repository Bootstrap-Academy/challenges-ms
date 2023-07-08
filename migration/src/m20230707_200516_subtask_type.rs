use std::collections::HashMap;

use sea_orm_migration::{prelude::*, sea_orm::prelude::Uuid, sea_query::extension::postgres::Type};

use crate::{
    m20230322_163425_challenges_init::{CodingChallenge, MultipleChoice, Subtask},
    m20230621_074711_questions::Question,
    m20230621_141228_matchings::Matching,
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_type(
                Type::create()
                    .as_enum(SubtaskType::Type)
                    .values([
                        SubtaskType::CodingChallenge,
                        SubtaskType::Matching,
                        SubtaskType::MultipleChoiceQuestion,
                        SubtaskType::Question,
                    ])
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Subtask::Table)
                    .add_column(
                        ColumnDef::new(Subtask::Type)
                            .custom(SubtaskType::Type)
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        async fn get_subtasks(
            manager: &SchemaManager<'_>,
            table: impl Iden + 'static,
            id_col: impl Iden + 'static,
        ) -> Result<Vec<Uuid>, DbErr> {
            let conn = manager.get_connection();
            let builder = conn.get_database_backend();
            let col_name = id_col.to_string();
            conn.query_all(builder.build(Query::select().from(table).column(id_col)))
                .await?
                .into_iter()
                .map(|row| row.try_get_by(col_name.as_str()))
                .collect::<Result<Vec<Uuid>, _>>()
        }

        let types = std::iter::empty()
            .chain(
                get_subtasks(manager, MultipleChoice::Table, MultipleChoice::SubtaskId)
                    .await?
                    .into_iter()
                    .map(|x| (x, SubtaskType::MultipleChoiceQuestion)),
            )
            .chain(
                get_subtasks(manager, Question::Table, Question::SubtaskId)
                    .await?
                    .into_iter()
                    .map(|x| (x, SubtaskType::Question)),
            )
            .chain(
                get_subtasks(manager, Matching::Table, Matching::SubtaskId)
                    .await?
                    .into_iter()
                    .map(|x| (x, SubtaskType::Matching)),
            )
            .chain(
                get_subtasks(manager, CodingChallenge::Table, CodingChallenge::SubtaskId)
                    .await?
                    .into_iter()
                    .map(|x| (x, SubtaskType::CodingChallenge)),
            )
            .collect::<HashMap<Uuid, SubtaskType>>();
        for (id, ty) in types {
            manager
                .exec_stmt(
                    Query::update()
                        .table(Subtask::Table)
                        .value(
                            Subtask::Type,
                            SimpleExpr::Constant(Value::String(Some(ty.to_string().into()))),
                        )
                        .and_where(Expr::col(Subtask::Id).eq(id))
                        .to_owned(),
                )
                .await?;
        }

        manager
            .alter_table(
                Table::alter()
                    .table(Subtask::Table)
                    .modify_column(
                        ColumnDef::new(Subtask::Type)
                            .custom(SubtaskType::Type)
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Subtask::Table)
                    .drop_column(Subtask::Type)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_type(Type::drop().name(SubtaskType::Type).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(Debug, Iden)]
pub enum SubtaskType {
    #[iden = "challenges_subtask_type"]
    Type,
    CodingChallenge,
    Matching,
    MultipleChoiceQuestion,
    Question,
}
