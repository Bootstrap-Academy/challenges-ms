use std::sync::Arc;

use chrono::{DateTime, Utc};
use entity::{
    challenges_multiple_choice_attempts, challenges_multiple_choice_quizes,
    challenges_user_subtasks,
};
use lib::{
    auth::{AdminAuth, VerifiedUserAuth},
    config::Config,
    SharedState,
};
use poem::web::Data;
use poem_ext::{db::DbTxn, patch_value::PatchValue, response};
use poem_openapi::{
    param::{Path, Query},
    payload::Json,
    OpenApi,
};
use sea_orm::{ActiveModelTrait, ColumnTrait, ModelTrait, QueryFilter, QueryOrder, Set, Unchanged};
use uuid::Uuid;

use super::Tags;
use crate::{
    schemas::multiple_choice::{
        check_answers, split_answers, Answer, CreateMultipleChoiceQuestionRequest,
        MultipleChoiceQuestion, MultipleChoiceQuestionSummary, SolveMCQFeedback, SolveMCQRequest,
        UpdateMultipleChoiceQuestionRequest,
    },
    services::subtasks::{
        create_subtask, get_subtask, get_user_subtask, query_subtask, query_subtask_admin,
        query_subtasks, send_task_rewards, update_subtask, update_user_subtask, CreateSubtaskError,
        QuerySubtaskError, QuerySubtasksFilter, UpdateSubtaskError, UserSubtaskExt,
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
    #[allow(clippy::too_many_arguments)]
    async fn list_questions(
        &self,
        task_id: Path<Uuid>,
        /// Whether to search for free questions.
        free: Query<Option<bool>>,
        /// Whether to search for unlocked questions.
        unlocked: Query<Option<bool>>,
        /// Whether to search for solved questions.
        solved: Query<Option<bool>>,
        /// Whether to search for rated questions.
        rated: Query<Option<bool>>,
        /// Whether to search for enabled subtasks.
        enabled: Query<Option<bool>>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> ListMCQs::Response<VerifiedUserAuth> {
        ListMCQs::ok(
            query_subtasks::<challenges_multiple_choice_quizes::Entity, _>(
                &db,
                &auth.0,
                task_id.0,
                QuerySubtasksFilter {
                    free: free.0,
                    unlocked: unlocked.0,
                    solved: solved.0,
                    rated: rated.0,
                    enabled: enabled.0,
                },
                MultipleChoiceQuestionSummary::from,
            )
            .await?,
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
    ) -> GetMCQ::Response<VerifiedUserAuth> {
        match query_subtask::<challenges_multiple_choice_quizes::Entity, _>(
            &db,
            &auth.0,
            task_id.0,
            subtask_id.0,
            MultipleChoiceQuestion::<String>::from,
        )
        .await?
        {
            Ok(mcq) => GetMCQ::ok(mcq),
            Err(QuerySubtaskError::NotFound) => GetMCQ::subtask_not_found(),
            Err(QuerySubtaskError::NoAccess) => GetMCQ::no_access(),
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
    ) -> GetMCQWithSolution::Response<VerifiedUserAuth> {
        match query_subtask_admin::<challenges_multiple_choice_quizes::Entity, _>(
            &db,
            &auth.0,
            task_id.0,
            subtask_id.0,
            MultipleChoiceQuestion::<Answer>::from,
        )
        .await?
        {
            Ok(mcq) => GetMCQWithSolution::ok(mcq),
            Err(QuerySubtaskError::NotFound) => GetMCQWithSolution::subtask_not_found(),
            Err(QuerySubtaskError::NoAccess) => GetMCQWithSolution::forbidden(),
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
    ) -> CreateMCQ::Response<VerifiedUserAuth> {
        let subtask = match create_subtask(
            &db,
            &self.state.services,
            &self.config,
            &auth.0,
            task_id.0,
            data.0.subtask,
        )
        .await?
        {
            Ok(subtask) => subtask,
            Err(CreateSubtaskError::TaskNotFound) => return CreateMCQ::task_not_found(),
            Err(CreateSubtaskError::Forbidden) => return CreateMCQ::forbidden(),
            Err(CreateSubtaskError::Banned(until)) => return CreateMCQ::banned(until),
            Err(CreateSubtaskError::XpLimitExceeded(x)) => return CreateMCQ::xp_limit_exceeded(x),
            Err(CreateSubtaskError::CoinLimitExceeded(x)) => {
                return CreateMCQ::coin_limit_exceeded(x)
            }
            Err(CreateSubtaskError::FeeLimitExceeded(x)) => {
                return CreateMCQ::fee_limit_exceeded(x)
            }
        };

        let correct_cnt = data.0.answers.iter().filter(|x| x.correct).count();
        if data.0.single_choice && correct_cnt != 1 {
            return CreateMCQ::invalid_single_choice();
        }
        if correct_cnt == 0 {
            return CreateMCQ::invalid_multiple_choice();
        }

        let (answers, correct) = split_answers(data.0.answers);
        let mcq = challenges_multiple_choice_quizes::ActiveModel {
            subtask_id: Set(subtask.id),
            question: Set(data.0.question),
            answers: Set(answers),
            correct_answers: Set(correct),
            single_choice: Set(data.0.single_choice),
        }
        .insert(&***db)
        .await?;
        CreateMCQ::ok(MultipleChoiceQuestion::<Answer>::from(mcq, subtask))
    }

    /// Update a multiple choice question.
    #[oai(path = "/tasks/:task_id/multiple_choice/:subtask_id", method = "patch")]
    async fn update_question(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        data: Json<UpdateMultipleChoiceQuestionRequest>,
        db: Data<&DbTxn>,
        auth: AdminAuth,
    ) -> UpdateMCQ::Response<AdminAuth> {
        let (mcq, subtask) = match update_subtask::<challenges_multiple_choice_quizes::Entity>(
            &db,
            &auth.0,
            task_id.0,
            subtask_id.0,
            data.0.subtask,
        )
        .await?
        {
            Ok(x) => x,
            Err(UpdateSubtaskError::SubtaskNotFound) => return UpdateMCQ::subtask_not_found(),
            Err(UpdateSubtaskError::TaskNotFound) => return UpdateMCQ::task_not_found(),
        };

        let (answers, correct, cnt) = if let PatchValue::Set(answers) = data.0.answers {
            let cnt = answers.iter().filter(|x| x.correct).count();
            let (a, c) = split_answers(answers);
            (Set(a), Set(c), cnt)
        } else {
            let cnt = mcq.correct_answers.count_ones() as _;
            (Unchanged(mcq.answers), Unchanged(mcq.correct_answers), cnt)
        };

        if *data.0.single_choice.get_new(&mcq.single_choice) && cnt != 1 {
            return UpdateMCQ::invalid_single_choice();
        }
        if cnt == 0 {
            return UpdateMCQ::invalid_multiple_choice();
        }

        let mcq = challenges_multiple_choice_quizes::ActiveModel {
            subtask_id: Unchanged(mcq.subtask_id),
            question: data.0.question.update(mcq.question),
            answers,
            correct_answers: correct,
            single_choice: data.0.single_choice.update(mcq.single_choice),
        }
        .update(&***db)
        .await?;

        UpdateMCQ::ok(MultipleChoiceQuestion::<Answer>::from(mcq, subtask))
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
        data: Json<SolveMCQRequest>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> SolveMCQ::Response<VerifiedUserAuth> {
        let Some((mcq, subtask)) = get_subtask::<challenges_multiple_choice_quizes::Entity>(&db, task_id.0, subtask_id.0).await? else {
            return SolveMCQ::subtask_not_found();
        };
        if !auth.0.admin && auth.0.id != subtask.creator && !subtask.enabled {
            return SolveMCQ::subtask_not_found();
        }

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;
        if !user_subtask.check_access(&auth.0, &subtask) {
            return SolveMCQ::no_access();
        }

        if data.0.answers.len() != mcq.answers.len() {
            return SolveMCQ::wrong_length();
        }

        let previous_attempts = mcq
            .find_related(challenges_multiple_choice_attempts::Entity)
            .filter(challenges_multiple_choice_attempts::Column::UserId.eq(auth.0.id))
            .order_by_desc(challenges_multiple_choice_attempts::Column::Timestamp)
            .all(&***db)
            .await?;
        let solved_previously = user_subtask.is_solved();
        if let Some(last_attempt) = previous_attempts.first() {
            let time_left = self
                .config
                .challenges
                .multiple_choice_questions
                .timeout_incr as i64
                * previous_attempts.len() as i64
                - (Utc::now().naive_utc() - last_attempt.timestamp).num_seconds();
            if !solved_previously && time_left > 0 {
                return SolveMCQ::too_many_requests(time_left as u64);
            }
        }

        let correct_cnt = check_answers(&data.0.answers, mcq.correct_answers);
        let solved = correct_cnt == mcq.answers.len();

        if !solved_previously {
            let now = Utc::now().naive_utc();
            if solved {
                update_user_subtask(
                    &db,
                    user_subtask.as_ref(),
                    challenges_user_subtasks::ActiveModel {
                        user_id: Set(auth.0.id),
                        subtask_id: Set(subtask.id),
                        unlocked_timestamp: user_subtask
                            .as_ref()
                            .and_then(|x| x.unlocked_timestamp)
                            .map(|x| Unchanged(Some(x)))
                            .unwrap_or(Set(Some(now))),
                        solved_timestamp: Set(Some(now)),
                        ..Default::default()
                    },
                )
                .await?;

                if auth.0.id != subtask.creator {
                    send_task_rewards(&self.state.services, &db, auth.0.id, &subtask).await?;
                }
            }

            challenges_multiple_choice_attempts::ActiveModel {
                id: Set(Uuid::new_v4()),
                question_id: Set(mcq.subtask_id),
                user_id: Set(auth.0.id),
                timestamp: Set(now),
                solved: Set(solved),
            }
            .insert(&***db)
            .await?;
        }

        SolveMCQ::ok(SolveMCQFeedback {
            solved,
            correct: correct_cnt,
        })
    }
}

response!(ListMCQs = {
    Ok(200) => Vec<MultipleChoiceQuestionSummary>,
});

response!(GetMCQ = {
    Ok(200) => MultipleChoiceQuestion<String>,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user has not unlocked this question.
    NoAccess(403, error),
});

response!(GetMCQWithSolution = {
    Ok(200) => MultipleChoiceQuestion<Answer>,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user is not allowed to view the solution to this question.
    Forbidden(403, error),
});

response!(CreateMCQ = {
    Ok(201) => MultipleChoiceQuestion<Answer>,
    /// Task does not exist.
    TaskNotFound(404, error),
    /// The user is not allowed to create questions in this task.
    Forbidden(403, error),
    /// The user is currently banned from creating subtasks.
    Banned(403, error) => Option<DateTime<Utc>>,
    /// The max xp limit has been exceeded.
    XpLimitExceeded(403, error) => u64,
    /// The max coin limit has been exceeded.
    CoinLimitExceeded(403, error) => u64,
    /// The max fee limit has been exceeded.
    FeeLimitExceeded(403, error) => u64,
    /// `single_choice` is set to `true`, but there is not exactly one correct answer.
    InvalidSingleChoice(400, error),
    /// There is no correct answer.
    InvalidMultipleChoice(400, error),
});

response!(UpdateMCQ = {
    Ok(200) => MultipleChoiceQuestion<Answer>,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// Task does not exist.
    TaskNotFound(404, error),
    /// `single_choice` is set to `true`, but there is not exactly one correct answer.
    InvalidSingleChoice(400, error),
    /// There is no correct answer.
    InvalidMultipleChoice(400, error),
});

response!(SolveMCQ = {
    Ok(201) => SolveMCQFeedback,
    /// Wrong number of answers.
    WrongLength(400, error),
    /// Try again later. `details` contains the number of seconds to wait.
    TooManyRequests(429, error) => u64,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user has not unlocked this question.
    NoAccess(403, error),
});
