use std::sync::Arc;

use lib::SharedState;
use poem_ext::response;
use poem_openapi::OpenApi;

use super::Tags;

pub struct CodingChallenges {
    pub state: Arc<SharedState>,
}

#[OpenApi(tag = "Tags::CodingChallenges")]
impl CodingChallenges {
    #[oai(path = "/test2", method = "get", hidden)]
    async fn test(&self) -> Test2::Response {
        Test2::ok()
    }
}

response!(Test2 = {
    Ok(200),
});
