use std::sync::Arc;

use entity::challenges_challenge_categories;
use lib::{
    auth::{AdminAuth, VerifiedUserAuth},
    SharedState,
};
use poem_ext::{response, responses::internal_server_error};
use poem_openapi::{
    param::{Path, Query},
    payload::Json,
    OpenApi,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, ModelTrait, QueryFilter,
    QueryOrder, Set, Unchanged,
};
use uuid::Uuid;

use crate::schemas::challenges::{Category, CreateCategoryRequest, UpdateCategoryRequest};

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
        _auth: VerifiedUserAuth,
    ) -> ListCategories::Response<VerifiedUserAuth> {
        let mut query = challenges_challenge_categories::Entity::find()
            .order_by_asc(challenges_challenge_categories::Column::Title);
        if let Some(title) = title.0 {
            query = query.filter(challenges_challenge_categories::Column::Title.contains(&title));
        }
        ListCategories::ok(
            query
                .all(&self.state.db)
                .await
                .map_err(internal_server_error)?
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
        _auth: VerifiedUserAuth,
    ) -> GetCategory::Response<VerifiedUserAuth> {
        match get_category(&self.state.db, category_id.0).await? {
            Some(category) => GetCategory::ok(category.into()),
            None => GetCategory::not_found(),
        }
    }

    /// Create a new challenge category.
    #[oai(path = "/categories", method = "post")]
    async fn create_category(
        &self,
        data: Json<CreateCategoryRequest>,
        _auth: AdminAuth,
    ) -> CreateCategory::Response<AdminAuth> {
        CreateCategory::ok(
            challenges_challenge_categories::ActiveModel {
                id: Set(Uuid::new_v4()),
                title: Set(data.0.title),
                description: Set(data.0.description),
            }
            .insert(&self.state.db)
            .await
            .map_err(internal_server_error)?
            .into(),
        )
    }

    /// Update a challenge category.
    #[oai(path = "/categories/:category_id", method = "patch")]
    async fn update_category(
        &self,
        category_id: Path<Uuid>,
        data: Json<UpdateCategoryRequest>,
        _auth: AdminAuth,
    ) -> UpdateCategory::Response<AdminAuth> {
        match get_category(&self.state.db, category_id.0).await? {
            Some(category) => UpdateCategory::ok(
                challenges_challenge_categories::ActiveModel {
                    id: Unchanged(category.id),
                    title: data.0.title.update(category.title),
                    description: data.0.description.update(category.description),
                }
                .update(&self.state.db)
                .await
                .map_err(internal_server_error)?
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
        _auth: AdminAuth,
    ) -> DeleteCategory::Response<AdminAuth> {
        match get_category(&self.state.db, category_id.0).await? {
            Some(category) => {
                category
                    .delete(&self.state.db)
                    .await
                    .map_err(internal_server_error)?;
                DeleteCategory::ok()
            }
            None => DeleteCategory::not_found(),
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

async fn get_category(
    db: &DatabaseConnection,
    category_id: Uuid,
) -> poem::Result<Option<challenges_challenge_categories::Model>> {
    Ok(
        challenges_challenge_categories::Entity::find_by_id(category_id)
            .one(db)
            .await
            .map_err(internal_server_error)?,
    )
}
