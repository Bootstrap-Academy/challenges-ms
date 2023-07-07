use entity::{challenges_challenges, challenges_course_tasks, challenges_tasks};
use sea_orm::{DatabaseTransaction, DbErr, EntityTrait, ModelTrait};
use uuid::Uuid;

pub async fn get_task(
    db: &DatabaseTransaction,
    task_id: Uuid,
) -> Result<Option<challenges_tasks::Model>, DbErr> {
    challenges_tasks::Entity::find_by_id(task_id).one(db).await
}

pub async fn get_task_with_specific(
    db: &DatabaseTransaction,
    task_id: Uuid,
) -> Result<Option<(challenges_tasks::Model, Task)>, DbErr> {
    Ok(
        match challenges_tasks::Entity::find_by_id(task_id)
            .one(db)
            .await?
        {
            Some(task) => get_specific_task(db, &task).await?.map(|x| (task, x)),
            None => None,
        },
    )
}

pub async fn get_specific_task(
    db: &DatabaseTransaction,
    task: &challenges_tasks::Model,
) -> Result<Option<Task>, DbErr> {
    if let Some(challenge) = task
        .find_related(challenges_challenges::Entity)
        .one(db)
        .await?
    {
        return Ok(Some(Task::Challenge(challenge)));
    }
    if let Some(course_task) = task
        .find_related(challenges_course_tasks::Entity)
        .one(db)
        .await?
    {
        return Ok(Some(Task::CourseTask(course_task)));
    }
    Ok(None)
}

#[derive(Debug)]
pub enum Task {
    Challenge(challenges_challenges::Model),
    CourseTask(challenges_course_tasks::Model),
}
