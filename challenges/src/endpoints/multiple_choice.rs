use std::sync::Arc;

use chrono::Utc;
use entity::{
    challenges_multiple_choice_attempts, challenges_multiple_choice_quizes, challenges_subtasks,
};
use lib::{
    auth::{AdminAuth, VerifiedUserAuth},
    config::Config,
    SharedState,
};
use poem::web::Data;
use poem_ext::{db::DbTxn, patch_value::PatchValue, response, responses::ErrorResponse};
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

use super::Tags;
use crate::{
    schemas::multiple_choice::{
        check_answers, split_answers, Answer, CreateMultipleChoiceQuestionRequest,
        MultipleChoiceQuestion, MultipleChoiceQuestionSummary, SolveQuestionFeedback,
        SolveQuestionRequest, UpdateMultipleChoiceQuestionRequest,
    },
    services::{
        subtasks::{can_create, check_unlocked, get_unlocked, send_task_rewards},
        tasks::{get_task, get_task_with_specific, Task},
    },
};

pub struct MultipleChoice {
    pub state: Arc<SharedState>,
    pub config: Arc<Config>,
}

#[OpenApi(tag = "Tags::MultipleChoice")]
impl MultipleChoice {
    /// List all multiple choice questions in a task.
    #[oai(path = "/tasks/:task_id/multiple_choice", method = "get")]
    async fn list_questions(
        &self,
        task_id: Path<Uuid>,
        /// Whether to search for free questions.
        free: Query<Option<bool>>,
        /// Whether to search for unlocked questions.
        unlocked: Query<Option<bool>>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> ListQuestions::Response<VerifiedUserAuth> {
        let unlocked_subtasks = get_unlocked(&db, auth.0.id).await?;
        ListQuestions::ok(
            challenges_multiple_choice_quizes::Entity::find()
                .find_also_related(challenges_subtasks::Entity)
                .filter(challenges_subtasks::Column::TaskId.eq(task_id.0))
                .order_by_asc(challenges_subtasks::Column::CreationTimestamp)
                .all(&***db)
                .await?
                .into_iter()
                .filter_map(|(mcq, subtask)| {
                    let subtask = subtask?;
                    let id = subtask.id;
                    let free_ = subtask.fee <= 0;
                    let unlocked_ = auth.0.admin
                        || auth.0.id == subtask.creator
                        || free_
                        || unlocked_subtasks.contains(&id);
                    (free.unwrap_or(free_) == free_ && unlocked.unwrap_or(unlocked_) == unlocked_)
                        .then_some(MultipleChoiceQuestionSummary::from(mcq, subtask, unlocked_))
                })
                .collect(),
        )
    }

    /// Get a multiple choice question by id.
    #[oai(path = "/tasks/:task_id/multiple_choice/:subtask_id", method = "get")]
    async fn get_question(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetQuestion::Response<VerifiedUserAuth> {
        match get_question(&db, task_id.0, subtask_id.0).await? {
            Some((mcq, subtask)) if check_unlocked(&db, &auth.0, &subtask).await? => {
                GetQuestion::ok(MultipleChoiceQuestion::<String>::from(mcq, subtask))
            }
            Some(_) => GetQuestion::no_access(),
            None => GetQuestion::subtask_not_found(),
        }
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
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetQuestionWithSolution::Response<VerifiedUserAuth> {
        match get_question(&db, task_id.0, subtask_id.0).await? {
            Some((mcq, subtask)) => {
                if auth.0.admin || auth.0.id == subtask.creator {
                    GetQuestionWithSolution::ok(MultipleChoiceQuestion::<Answer>::from(
                        mcq, subtask,
                    ))
                } else {
                    GetQuestionWithSolution::forbidden()
                }
            }
            None => GetQuestionWithSolution::subtask_not_found(),
        }
    }

    /// Create a new multiple choice question.
    #[oai(path = "/tasks/:task_id/multiple_choice", method = "post")]
    async fn create_question(
        &self,
        task_id: Path<Uuid>,
        data: Json<CreateMultipleChoiceQuestionRequest>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> CreateQuestion::Response<VerifiedUserAuth> {
        let (task, specific) = match get_task_with_specific(&db, task_id.0).await? {
            Some(task) => task,
            None => return CreateQuestion::task_not_found(),
        };
        if !can_create(&self.state.services, &self.config, &specific, &auth.0).await? {
            return CreateQuestion::forbidden();
        }

        if matches!(specific, Task::CourseTask(_))
            && data.0.fee > self.config.challenges.quizzes.max_fee
            && !auth.0.admin
        {
            return CreateQuestion::fee_limit_exceeded(self.config.challenges.quizzes.max_fee);
        }

        let subtask = challenges_subtasks::ActiveModel {
            id: Set(Uuid::new_v4()),
            task_id: Set(task.id),
            creator: Set(auth.0.id),
            creation_timestamp: Set(Utc::now().naive_utc()),
            xp: Set(data.0.xp),
            coins: Set(data.0.coins),
            fee: Set(data.0.fee as _),
        }
        .insert(&***db)
        .await?;
        let (answers, correct) = split_answers(data.0.answers);
        let mcq = challenges_multiple_choice_quizes::ActiveModel {
            subtask_id: Set(subtask.id),
            question: Set(data.0.question),
            answers: Set(answers),
            correct_answers: Set(correct),
        }
        .insert(&***db)
        .await?;
        CreateQuestion::ok(MultipleChoiceQuestion::<Answer>::from(mcq, subtask))
    }

    /// Update a multiple choice question.
    #[oai(path = "/tasks/:task_id/multiple_choice/:subtask_id", method = "patch")]
    async fn update_question(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        data: Json<UpdateMultipleChoiceQuestionRequest>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> UpdateQuestion::Response<AdminAuth> {
        match get_question(&db, task_id.0, subtask_id.0).await? {
            Some((mcq, subtask)) => {
                if get_task(&db, *data.0.task_id.get_new(&subtask.task_id))
                    .await?
                    .is_none()
                {
                    return UpdateQuestion::task_not_found();
                };
                let (answers, correct) = if let PatchValue::Set(answers) = data.0.answers {
                    let (a, c) = split_answers(answers);
                    (Set(a), Set(c))
                } else {
                    (Unchanged(mcq.answers), Unchanged(mcq.correct_answers))
                };
                let mcq = challenges_multiple_choice_quizes::ActiveModel {
                    subtask_id: Unchanged(mcq.subtask_id),
                    question: data.0.question.update(mcq.question),
                    answers,
                    correct_answers: correct,
                }
                .update(&***db)
                .await?;
                let subtask = challenges_subtasks::ActiveModel {
                    id: Unchanged(subtask.id),
                    task_id: data.0.task_id.update(subtask.task_id),
                    creator: Unchanged(subtask.creator),
                    creation_timestamp: Unchanged(subtask.creation_timestamp),
                    xp: data.0.xp.update(subtask.xp),
                    coins: data.0.coins.update(subtask.coins),
                    fee: data.0.fee.map(|x| x as _).update(subtask.fee),
                }
                .update(&***db)
                .await?;
                UpdateQuestion::ok(MultipleChoiceQuestion::<Answer>::from(mcq, subtask))
            }
            None => UpdateQuestion::subtask_not_found(),
        }
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
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> DeleteQuestion::Response<AdminAuth> {
        match get_question(&db, task_id.0, subtask_id.0).await? {
            Some((_, subtask)) => {
                subtask.delete(&***db).await?;
                DeleteQuestion::ok()
            }
            None => DeleteQuestion::subtask_not_found(),
        }
    }

    /// Attempt to solve a multiple choice question.
    #[oai(
        path = "/tasks/:task_id/multiple_choice/:subtask_id/attempts",
        method = "post"
    )]
    async fn solve_question(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        data: Json<SolveQuestionRequest>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> SolveQuestion::Response<VerifiedUserAuth> {
        let Some((mcq, subtask)) = get_question(&db, task_id.0, subtask_id.0).await? else {
            return SolveQuestion::subtask_not_found();
        };
        if !check_unlocked(&db, &auth.0, &subtask).await? {
            return SolveQuestion::no_access();
        }
        if data.0.answers.len() != mcq.answers.len() {
            return SolveQuestion::wrong_length();
        }

        let previous_attempts = mcq
            .find_related(challenges_multiple_choice_attempts::Entity)
            .filter(challenges_multiple_choice_attempts::Column::UserId.eq(auth.0.id))
            .order_by_desc(challenges_multiple_choice_attempts::Column::Timestamp)
            .all(&***db)
            .await?;
        let solved_previously = previous_attempts.iter().any(|a| a.solved);
        if let Some(last_attempt) = previous_attempts.first() {
            let time_left = self
                .config
                .challenges
                .multiple_choice_questions
                .timeout_incr as i64
                * previous_attempts.len() as i64
                - (Utc::now().naive_utc() - last_attempt.timestamp).num_seconds();
            if !solved_previously && time_left > 0 {
                return SolveQuestion::too_many_requests(time_left as u64);
            }
        }

        let correct = check_answers(&data.0.answers, mcq.correct_answers);
        let solved = correct == mcq.answers.len();

        if !solved_previously {
            if solved {
                send_task_rewards(&self.state.services, &db, auth.0.id, &subtask).await?;
            }

            challenges_multiple_choice_attempts::ActiveModel {
                id: Set(Uuid::new_v4()),
                question_id: Set(mcq.subtask_id),
                user_id: Set(auth.0.id),
                timestamp: Set(Utc::now().naive_utc()),
                solved: Set(solved),
            }
            .insert(&***db)
            .await?;
        }

        SolveQuestion::ok(SolveQuestionFeedback { solved, correct })
    }
}

response!(ListQuestions = {
    Ok(200) => Vec<MultipleChoiceQuestionSummary>,
});

response!(GetQuestion = {
    Ok(200) => MultipleChoiceQuestion<String>,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user has not unlocked this question.
    NoAccess(403, error),
});

response!(GetQuestionWithSolution = {
    Ok(200) => MultipleChoiceQuestion<Answer>,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user is not allowed to view the solution to this question.
    Forbidden(403, error),
});

response!(CreateQuestion = {
    Ok(201) => MultipleChoiceQuestion<Answer>,
    /// Task does not exist.
    TaskNotFound(404, error),
    /// The user is not allowed to create questions in this task.
    Forbidden(403, error),
    /// The max fee limit has been exceeded.
    FeeLimitExceeded(403, error) => u64,
});

response!(UpdateQuestion = {
    Ok(200) => MultipleChoiceQuestion<Answer>,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// Task does not exist.
    TaskNotFound(404, error),
});

response!(DeleteQuestion = {
    Ok(200),
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
});

response!(SolveQuestion = {
    Ok(201) => SolveQuestionFeedback,
    /// Wrong number of answers.
    WrongLength(400, error),
    /// Try again later. `details` contains the number of seconds to wait.
    TooManyRequests(429, error) => u64,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user has not unlocked this question.
    NoAccess(403, error),
});

async fn get_question(
    db: &DatabaseTransaction,
    task_id: Uuid,
    subtask_id: Uuid,
) -> Result<
    Option<(
        challenges_multiple_choice_quizes::Model,
        challenges_subtasks::Model,
    )>,
    ErrorResponse,
> {
    Ok(
        match challenges_multiple_choice_quizes::Entity::find_by_id(subtask_id)
            .find_also_related(challenges_subtasks::Entity)
            .filter(challenges_subtasks::Column::TaskId.eq(task_id))
            .one(db)
            .await?
        {
            Some((mcq, Some(subtask))) => Some((mcq, subtask)),
            _ => None,
        },
    )
}
