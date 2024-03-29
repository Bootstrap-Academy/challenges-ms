//! `SeaORM` Entity. Generated by sea-orm-codegen 0.12.2

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "challenges_challenge_categories")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(column_type = "Text")]
    pub title: String,
    #[sea_orm(column_type = "Text")]
    pub description: String,
    pub creation_timestamp: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::challenges_challenges::Entity")]
    ChallengesChallenges,
}

impl Related<super::challenges_challenges::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ChallengesChallenges.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
