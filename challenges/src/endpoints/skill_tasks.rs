use std::sync::Arc;

use lib::SharedState;
use poem_ext::response;
use poem_openapi::OpenApi;

use super::Tags;

pub struct SkillTasks {
    pub state: Arc<SharedState>,
}

#[OpenApi(tag = "Tags::SkillTasks")]
impl SkillTasks {
    #[oai(path = "/test4", method = "get")]
    async fn test(&self) -> Test4::Response {
        Test4::ok()
    }
}

response!(Test4 = {
    Ok(200),
});
