use std::sync::Arc;

use lib::SharedState;
use poem_ext::response;
use poem_openapi::OpenApi;

use super::Tags;

pub struct Challenges {
    pub state: Arc<SharedState>,
}

#[OpenApi(tag = "Tags::Challenges")]
impl Challenges {
    #[oai(path = "/test", method = "get")]
    async fn test(&self) -> Test::Response {
        Test::ok()
    }
}

response!(Test = {
    Ok(200),
});
