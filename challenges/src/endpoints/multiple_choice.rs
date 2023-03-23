use std::sync::Arc;

use entity::{challenges_multiple_choice_quizes, challenges_subtasks};
use lib::{
    auth::{AdminAuth, VerifiedUserAuth},
    SharedState,
};
use poem_ext::{response, responses::internal_server_error};
use poem_openapi::{param::Path, payload::Json, OpenApi};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::schemas::multiple_choice::{
    Answer, CreateMultipleChoiceQuestionRequest, MultipleChoiceQuestion,
};

use super::Tags;

pub struct MultipleChoice {
    pub state: Arc<SharedState>,
}

#[OpenApi(tag = "Tags::MultipleChoice")]
impl MultipleChoice {
    /// List all multiple choice questions in a task.
    #[oai(path = "/tasks/:task_id/multiple_choice", method = "get")]
    async fn list_questions(
        &self,
        task_id: Path<Uuid>,
        _auth: VerifiedUserAuth,
    ) -> ListQuestions::Response<VerifiedUserAuth> {
        todo!()
    }

    /// Get a multiple choice question by id.
    #[oai(path = "/tasks/:task_id/multiple_choice/:subtask_id", method = "get")]
    async fn get_question(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        _auth: VerifiedUserAuth,
    ) -> GetQuestion::Response<VerifiedUserAuth> {
        todo!()
    }

    /// Get a multiple choice question and its solution by id.
    #[oai(
        path = "/tasks/:task_id/multiple_choice/:subtask_id/solution",
        method = "get"
    )]
    async fn get_question_with_solution(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        _auth: AdminAuth,
    ) -> GetQuestionWithSolution::Response<AdminAuth> {
        todo!()
    }

    /// Create a new multiple choice question.
    #[oai(path = "/tasks/:task_id/multiple_choice", method = "post")]
    async fn create_question(
        &self,
        task_id: Path<Uuid>,
        data: Json<CreateMultipleChoiceQuestionRequest>,
        _auth: AdminAuth,
    ) -> CreateQuestion::Response<AdminAuth> {
        todo!()
    }

    /// Update a multiple choice question.
    #[oai(path = "/tasks/:task_id/multiple_choice/:subtask_id", method = "patch")]
    async fn update_question(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        data: Json<CreateMultipleChoiceQuestionRequest>,
        _auth: AdminAuth,
    ) -> UpdateQuestion::Response<AdminAuth> {
        todo!()
    }

    /// Delete a multiple choice question.
    #[oai(
        path = "/tasks/:task_id/multiple_choice/:subtask_id",
        method = "delete"
    )]
    async fn delete_question(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        _auth: AdminAuth,
    ) -> DeleteQuestion::Response<AdminAuth> {
        todo!()
    }
}

response!(ListQuestions = {
    Ok(200) => Vec<MultipleChoiceQuestion<String>>,
});

response!(GetQuestion = {
    Ok(200) => MultipleChoiceQuestion<String>,
    /// Subtask does not exist.
    NotFound(404, error),
});

response!(GetQuestionWithSolution = {
    Ok(200) => MultipleChoiceQuestion<Answer>,
    /// Subtask does not exist.
    NotFound(404, error),
});

response!(CreateQuestion = {
    Ok(201) => MultipleChoiceQuestion<Answer>,
});

response!(UpdateQuestion = {
    Ok(200) => MultipleChoiceQuestion<Answer>,
    /// Subtask does not exist.
    NotFound(404, error),
});

response!(DeleteQuestion = {
    Ok(200),
    /// Subtask does not exist.
    NotFound(404, error),
});

async fn get_question(
    db: &DatabaseConnection,
    task_id: Uuid,
    subtask_id: Uuid,
) -> poem::Result<
    Option<(
        challenges_multiple_choice_quizes::Model,
        challenges_subtasks::Model,
    )>,
> {
    Ok(
        match challenges_multiple_choice_quizes::Entity::find_by_id(subtask_id)
            .find_also_related(challenges_subtasks::Entity)
            .filter(challenges_subtasks::Column::TaskId.eq(task_id))
            .one(db)
            .await
            .map_err(internal_server_error)?
        {
            Some((mcq, Some(subtask))) => Some((mcq, subtask)),
            _ => None,
        },
    )
}
