use std::sync::Arc;

use lib::SharedState;
use poem_ext::response;
use poem_openapi::OpenApi;

use super::Tags;

pub struct MultipleChoice {
    pub state: Arc<SharedState>,
}

#[OpenApi(tag = "Tags::MultipleChoice")]
impl MultipleChoice {
    #[oai(path = "/test3", method = "get")]
    async fn test(&self) -> Test3::Response {
        Test3::ok()
    }
}

response!(Test3 = {
    Ok(200),
});
