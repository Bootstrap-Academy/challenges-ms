use std::sync::Arc;

use lib::{auth::InternalAuth, SharedState};
use poem::web::Data;
use poem_ext::{db::DbTxn, response, responses::Response};
use poem_openapi::{param::Path, ApiResponse, OpenApi};
use schemas::challenges::user_export::UserDataExport;
use tracing::info;
use uuid::Uuid;

use super::Tags;
use crate::services::users::{delete_user_data, export_user_data};

pub struct Internal {
    pub state: Arc<SharedState>,
}

#[OpenApi(tag = "Tags::Internal")]
impl Internal {
    /// Return all data that belongs to a user.
    ///
    /// The export is empty for a user without any data in this service.
    #[oai(path = "/_internal/users/:user_id/export", method = "get")]
    async fn export_user(
        &self,
        user_id: Path<Uuid>,
        db: Data<&DbTxn>,
        _auth: InternalAuth,
    ) -> ExportUser::Response<InternalAuth> {
        ExportUser::ok(export_user_data(&db, user_id.0).await?)
    }

    /// Delete all data that belongs to a user.
    ///
    /// This endpoint is idempotent, deleting a user without any data is a
    /// success.
    #[oai(path = "/_internal/users/:user_id", method = "delete")]
    async fn delete_user(
        &self,
        user_id: Path<Uuid>,
        db: Data<&DbTxn>,
        _auth: InternalAuth,
    ) -> Response<DeleteUser, InternalAuth> {
        let rows = delete_user_data(&db, user_id.0).await?;
        self.state.cache.pop_tag(&format!("{}", user_id.0)).await?;
        info!("Deleted {rows} rows of a user");
        Ok(DeleteUser::NoContent.into())
    }
}

#[derive(Debug, ApiResponse)]
pub enum DeleteUser {
    /// All data of the user has been deleted.
    #[oai(status = 204)]
    NoContent,
}

response!(ExportUser = {
    /// All data of the user.
    Ok(200) => UserDataExport,
});
