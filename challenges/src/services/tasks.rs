use entity::challenges_tasks;
use poem_ext::responses::ErrorResponse;
use sea_orm::{DatabaseTransaction, EntityTrait};
use uuid::Uuid;

pub async fn get_task(
    db: &DatabaseTransaction,
    task_id: Uuid,
) -> Result<Option<challenges_tasks::Model>, ErrorResponse> {
    Ok(challenges_tasks::Entity::find_by_id(task_id)
        .one(db)
        .await?)
}
