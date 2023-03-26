use std::sync::Arc;

use chrono::Utc;
use entity::{challenges_challenge_categories, challenges_challenges, challenges_tasks};
use lib::{
    auth::{AdminAuth, VerifiedUserAuth},
    SharedState,
};
use poem::web::Data;
use poem_ext::{db::DbTxn, response, responses::ErrorResponse};
use poem_openapi::{
    param::{Path, Query},
    payload::Json,
    OpenApi,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, ModelTrait, QueryFilter,
    QueryOrder, Set, Unchanged,
};
use uuid::Uuid;

use crate::schemas::challenges::{
    Category, Challenge, CreateCategoryRequest, CreateChallengeRequest, UpdateCategoryRequest,
    UpdateChallengeRequest,
};

use super::Tags;

pub struct Challenges {
    pub state: Arc<SharedState>,
}

#[OpenApi(tag = "Tags::Challenges")]
impl Challenges {
    /// List all challenge categories.
    #[oai(path = "/categories", method = "get")]
    async fn list_categories(
        &self,
        /// Filter by category title
        title: Query<Option<String>>,
        db: Data<&DbTxn>,
        _auth: VerifiedUserAuth,
    ) -> ListCategories::Response<VerifiedUserAuth> {
        let mut query = challenges_challenge_categories::Entity::find()
            .order_by_asc(challenges_challenge_categories::Column::Title);
        if let Some(title) = title.0 {
            query = query.filter(challenges_challenge_categories::Column::Title.contains(&title));
        }
        ListCategories::ok(
            query
                .all(&***db)
                .await?
                .into_iter()
                .map(Into::into)
                .collect(),
        )
    }

    /// Get a challenge category by id.
    #[oai(path = "/categories/:category_id", method = "get")]
    async fn get_category(
        &self,
        category_id: Path<Uuid>,
        db: Data<&DbTxn>,
        _auth: VerifiedUserAuth,
    ) -> GetCategory::Response<VerifiedUserAuth> {
        match get_category(&db, category_id.0).await? {
            Some(category) => GetCategory::ok(category.into()),
            None => GetCategory::not_found(),
        }
    }

    /// Create a new challenge category.
    #[oai(path = "/categories", method = "post")]
    async fn create_category(
        &self,
        data: Json<CreateCategoryRequest>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> CreateCategory::Response<AdminAuth> {
        CreateCategory::ok(
            challenges_challenge_categories::ActiveModel {
                id: Set(Uuid::new_v4()),
                title: Set(data.0.title),
                description: Set(data.0.description),
            }
            .insert(&***db)
            .await?
            .into(),
        )
    }

    /// Update a challenge category.
    #[oai(path = "/categories/:category_id", method = "patch")]
    async fn update_category(
        &self,
        category_id: Path<Uuid>,
        data: Json<UpdateCategoryRequest>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> UpdateCategory::Response<AdminAuth> {
        match get_category(&db, category_id.0).await? {
            Some(category) => UpdateCategory::ok(
                challenges_challenge_categories::ActiveModel {
                    id: Unchanged(category.id),
                    title: data.0.title.update(category.title),
                    description: data.0.description.update(category.description),
                }
                .update(&***db)
                .await?
                .into(),
            ),
            None => UpdateCategory::not_found(),
        }
    }

    /// Delete a challenge category.
    ///
    /// This will also delete all challenges within this category!
    #[oai(path = "/categories/:category_id", method = "delete")]
    async fn delete_category(
        &self,
        category_id: Path<Uuid>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> DeleteCategory::Response<AdminAuth> {
        match get_category(&db, category_id.0).await? {
            Some(category) => {
                category.delete(&***db).await?;
                DeleteCategory::ok()
            }
            None => DeleteCategory::not_found(),
        }
    }

    /// List all challenges in a category.
    #[oai(path = "/categories/:category_id/challenges", method = "get")]
    async fn list_challenges(
        &self,
        category_id: Path<Uuid>,
        /// Filter by challenge title
        title: Query<Option<String>>,
        db: Data<&DbTxn>,
        _auth: VerifiedUserAuth,
    ) -> ListChallenges::Response<VerifiedUserAuth> {
        let mut query = challenges_challenges::Entity::find()
            .find_also_related(challenges_tasks::Entity)
            .filter(challenges_challenges::Column::CategoryId.eq(category_id.0))
            .order_by_asc(challenges_tasks::Column::Title);
        if let Some(title) = title.0 {
            query = query.filter(challenges_tasks::Column::Title.contains(&title));
        }
        ListChallenges::ok(
            query
                .all(&***db)
                .await?
                .into_iter()
                .filter_map(|(challenge, task)| Some(Challenge::from(challenge, task?)))
                .collect(),
        )
    }

    /// Get a challenge by id.
    #[oai(
        path = "/categories/:category_id/challenges/:challenge_id",
        method = "get"
    )]
    async fn get_challenge(
        &self,
        category_id: Path<Uuid>,
        challenge_id: Path<Uuid>,
        db: Data<&DbTxn>,
        _auth: VerifiedUserAuth,
    ) -> GetChallenge::Response<VerifiedUserAuth> {
        match get_challenge(&db, category_id.0, challenge_id.0).await? {
            Some((challenge, task)) => GetChallenge::ok(Challenge::from(challenge, task)),
            None => GetChallenge::challenge_not_found(),
        }
    }

    /// Create a new challenge.
    #[oai(path = "/categories/:category_id/challenges", method = "post")]
    async fn create_challenge(
        &self,
        category_id: Path<Uuid>,
        data: Json<CreateChallengeRequest>,
        db: Data<&DbTxn>,
        auth: AdminAuth,
    ) -> CreateChallenge::Response<AdminAuth> {
        let category = match get_category(&db, category_id.0).await? {
            Some(category) => category,
            None => return CreateChallenge::category_not_found(),
        };

        let task = challenges_tasks::ActiveModel {
            id: Set(Uuid::new_v4()),
            title: Set(data.0.title),
            description: Set(data.0.description),
            creator: Set(auth.0.id),
            creation_timestamp: Set(Utc::now().naive_utc()),
        }
        .insert(&***db)
        .await?;

        let challenge = challenges_challenges::ActiveModel {
            task_id: Set(task.id),
            category_id: Set(category.id),
        }
        .insert(&***db)
        .await?;

        CreateChallenge::ok(Challenge::from(challenge, task))
    }

    /// Update a challenge.
    #[oai(
        path = "/categories/:category_id/challenges/:challenge_id",
        method = "patch"
    )]
    async fn update_challenge(
        &self,
        category_id: Path<Uuid>,
        challenge_id: Path<Uuid>,
        data: Json<UpdateChallengeRequest>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> UpdateChallenge::Response<AdminAuth> {
        match get_challenge(&db, category_id.0, challenge_id.0).await? {
            Some((challenge, task)) => {
                if get_category(&db, *data.0.category.get_new(&challenge.category_id))
                    .await?
                    .is_none()
                {
                    return UpdateChallenge::category_not_found();
                }
                let challenge = challenges_challenges::ActiveModel {
                    task_id: Unchanged(challenge.task_id),
                    category_id: data.0.category.update(challenge.category_id),
                }
                .update(&***db)
                .await?;
                let task = challenges_tasks::ActiveModel {
                    id: Unchanged(task.id),
                    title: data.0.title.update(task.title),
                    description: data.0.description.update(task.description),
                    creator: Unchanged(task.creator),
                    creation_timestamp: Unchanged(task.creation_timestamp),
                }
                .update(&***db)
                .await?;
                UpdateChallenge::ok(Challenge::from(challenge, task))
            }
            None => UpdateChallenge::challenge_not_found(),
        }
    }

    /// Delete a challenge.
    #[oai(
        path = "/categories/:category_id/challenges/:challenge_id",
        method = "delete"
    )]
    async fn delete_challenge(
        &self,
        category_id: Path<Uuid>,
        challenge_id: Path<Uuid>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> DeleteChallenge::Response<AdminAuth> {
        match get_challenge(&db, category_id.0, challenge_id.0).await? {
            Some((_, task)) => {
                task.delete(&***db).await?;
                DeleteChallenge::ok()
            }
            None => DeleteChallenge::challenge_not_found(),
        }
    }
}

response!(ListCategories = {
    Ok(200) => Vec<Category>,
});

response!(GetCategory = {
    Ok(200) => Category,
    /// Category does not exist.
    NotFound(404, error),
});

response!(CreateCategory = {
    Ok(201) => Category,
});

response!(UpdateCategory = {
    Ok(200) => Category,
    /// Category does not exist.
    NotFound(404, error),
});

response!(DeleteCategory = {
    Ok(200),
    /// Category does not exist.
    NotFound(404, error),
});

response!(ListChallenges = {
    Ok(200) => Vec<Challenge>,
});

response!(GetChallenge = {
    Ok(200) => Challenge,
    /// Challenge does not exist.
    ChallengeNotFound(404, error),
});

response!(CreateChallenge = {
    Ok(201) => Challenge,
    /// Category does not exist.
    CategoryNotFound(404, error),
});

response!(UpdateChallenge = {
    Ok(200) => Challenge,
    /// Challenge does not exist.
    ChallengeNotFound(404, error),
    /// Category does not exist.
    CategoryNotFound(404, error),
});

response!(DeleteChallenge = {
    Ok(200),
    /// Challenge does not exist.
    ChallengeNotFound(404, error),
});

async fn get_category(
    db: &DatabaseTransaction,
    category_id: Uuid,
) -> Result<Option<challenges_challenge_categories::Model>, ErrorResponse> {
    Ok(
        challenges_challenge_categories::Entity::find_by_id(category_id)
            .one(db)
            .await?,
    )
}

async fn get_challenge(
    db: &DatabaseTransaction,
    category_id: Uuid,
    challenge_id: Uuid,
) -> Result<Option<(challenges_challenges::Model, challenges_tasks::Model)>, ErrorResponse> {
    Ok(
        match challenges_challenges::Entity::find_by_id(challenge_id)
            .find_also_related(challenges_tasks::Entity)
            .filter(challenges_challenges::Column::CategoryId.eq(category_id))
            .one(db)
            .await?
        {
            Some((challenge, Some(task))) => Some((challenge, task)),
            _ => None,
        },
    )
}
