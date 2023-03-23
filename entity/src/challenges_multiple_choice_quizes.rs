//! `SeaORM` Entity. Generated by sea-orm-codegen 0.11.0

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "challenges_multiple_choice_quizes")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub subtask_id: Uuid,
    #[sea_orm(column_type = "Text")]
    pub question: String,
    pub answers: Vec<String>,
    pub correct_answers: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::challenges_subtasks::Entity",
        from = "Column::SubtaskId",
        to = "super::challenges_subtasks::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    ChallengesSubtasks,
}

impl Related<super::challenges_subtasks::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ChallengesSubtasks.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}