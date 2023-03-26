use std::sync::Arc;

use crate::schemas::companies::{Company, CreateCompanyRequest, UpdateCompanyRequest};

use super::Tags;
use entity::jobs_companies;
use lib::{auth::AdminAuth, SharedState};
use poem::web::Data;
use poem_ext::{db::DbTxn, response, responses::ErrorResponse};
use poem_openapi::{param::Path, payload::Json, OpenApi};
use sea_orm::{ActiveModelTrait, DatabaseTransaction, EntityTrait, ModelTrait, Set, Unchanged};
use uuid::Uuid;

pub struct Companies {
    pub state: Arc<SharedState>,
}

#[OpenApi(tag = "Tags::Companies")]
impl Companies {
    /// List all companies.
    #[oai(path = "/companies", method = "get")]
    async fn list_companies(
        &self,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> ListCompanies::Response<AdminAuth> {
        ListCompanies::ok(
            jobs_companies::Entity::find()
                .all(&***db)
                .await?
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>(),
        )
    }

    /// Create a company.
    #[oai(path = "/companies", method = "post")]
    async fn create_company(
        &self,
        data: Json<CreateCompanyRequest>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> CreateCompany::Response<AdminAuth> {
        CreateCompany::ok(
            jobs_companies::ActiveModel {
                id: Set(Uuid::new_v4()),
                name: Set(data.0.name),
                description: Set(data.0.description),
                website: Set(data.0.website),
                youtube_video: Set(data.0.youtube_video),
                twitter_handle: Set(data.0.twitter_handle),
                instagram_handle: Set(data.0.instagram_handle),
                logo_url: Set(data.0.logo_url),
            }
            .insert(&***db)
            .await?
            .into(),
        )
    }

    /// Update a company.
    #[oai(path = "/companies/:company_id", method = "patch")]
    async fn update_company(
        &self,
        company_id: Path<Uuid>,
        data: Json<UpdateCompanyRequest>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> UpdateCompany::Response<AdminAuth> {
        match get_company(&db, company_id.0).await? {
            Some(company) => UpdateCompany::ok(
                jobs_companies::ActiveModel {
                    id: Unchanged(company.id),
                    name: data.0.name.update(company.name),
                    description: data.0.description.update(company.description),
                    website: data.0.website.update(company.website),
                    youtube_video: data.0.youtube_video.update(company.youtube_video),
                    twitter_handle: data.0.twitter_handle.update(company.twitter_handle),
                    instagram_handle: data.0.instagram_handle.update(company.instagram_handle),
                    logo_url: data.0.logo_url.update(company.logo_url),
                }
                .update(&***db)
                .await?
                .into(),
            ),
            None => UpdateCompany::not_found(),
        }
    }

    /// Delete a company.
    #[oai(path = "/companies/:company_id", method = "delete")]
    async fn delete_company(
        &self,
        company_id: Path<Uuid>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> DeleteCompany::Response<AdminAuth> {
        match get_company(&db, company_id.0).await? {
            Some(company) => {
                company.delete(&***db).await?;
                DeleteCompany::ok()
            }
            None => DeleteCompany::not_found(),
        }
    }
}

async fn get_company(
    db: &DatabaseTransaction,
    company_id: Uuid,
) -> Result<Option<jobs_companies::Model>, ErrorResponse> {
    Ok(jobs_companies::Entity::find_by_id(company_id)
        .one(db)
        .await?)
}

response!(ListCompanies = {
    Ok(200) => Vec<Company>,
});

response!(CreateCompany = {
    /// Company has been created successfully
    Ok(201) => Company,
});

response!(UpdateCompany = {
    /// Company has been updated successfully
    Ok(200) => Company,
    /// Company does not exist
    NotFound(404, error),
});

response!(DeleteCompany = {
    /// Company has been deleted successfully
    Ok(200),
    /// Company does not exist
    NotFound(404, error),
});
