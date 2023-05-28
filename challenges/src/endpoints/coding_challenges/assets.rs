use poem_openapi::{payload::PlainText, OpenApi};

use crate::{
    endpoints::Tags,
    services::judge::{EVALUATOR_LIBRARY, EVALUATOR_TEMPLATE},
};

pub struct Api;

#[OpenApi(tag = "Tags::CodingChallenges")]
impl Api {
    /// Return the evaluator template.
    #[oai(path = "/coding_challenges/evaluator/template.py", method = "get")]
    async fn get_evaluator_template(&self) -> PlainText<&'static str> {
        PlainText(EVALUATOR_TEMPLATE)
    }

    /// Return the evaluator library.
    #[oai(path = "/coding_challenges/evaluator/lib.py", method = "get")]
    async fn get_evaluator_lib(&self) -> PlainText<&'static str> {
        PlainText(EVALUATOR_LIBRARY)
    }
}
