use std::sync::Arc;

use chrono::{DateTime, Utc};
use entity::{
    challenges_questions, challenges_user_subtasks, sea_orm_active_enums::ChallengesSubtaskType,
};
use lib::{
    auth::{AdminAuth, VerifiedUserAuth},
    config::Config,
    SharedState,
};
use poem::web::Data;
use poem_ext::{db::DbTxn, response};
use poem_openapi::{
    param::{Path, Query},
    payload::Json,
    OpenApi,
};
use schemas::challenges::question::{
    CreateQuestionRequest, Question, QuestionSummary, QuestionWithSolution, SolveQuestionFeedback,
    SolveQuestionRequest, UpdateQuestionRequest,
};
use sea_orm::{ActiveModelTrait, Set, Unchanged};
use uuid::Uuid;

use super::Tags;
use crate::services::subtasks::{
    create_subtask, deduct_hearts, get_subtask, get_user_subtask, query_subtask,
    query_subtask_admin, query_subtasks, send_task_rewards, update_subtask, update_user_subtask,
    CreateSubtaskError, QuerySubtaskAdminError, QuerySubtasksFilter, UpdateSubtaskError,
    UserSubtaskExt,
};

pub struct Questions {
    pub state: Arc<SharedState>,
    pub config: Arc<Config>,
}

#[OpenApi(tag = "Tags::Questions")]
impl Questions {
    /// List all questions in a task.
    #[oai(path = "/tasks/:task_id/questions", method = "get")]
    #[allow(clippy::too_many_arguments)]
    async fn list_questions(
        &self,
        task_id: Path<Uuid>,
        /// Whether to search for subtasks the user has attempted to solve.
        attempted: Query<Option<bool>>,
        /// Whether to search for solved subtasks.
        solved: Query<Option<bool>>,
        /// Whether to search for rated subtasks.
        rated: Query<Option<bool>>,
        /// Whether to search for enabled subtasks.
        enabled: Query<Option<bool>>,
        /// Whether to search for retired subtasks.
        retired: Query<Option<bool>>,
        /// Filter by creator.
        creator: Query<Option<Uuid>>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> ListQuestions::Response<VerifiedUserAuth> {
        ListQuestions::ok(
            query_subtasks::<challenges_questions::Entity, _>(
                &db,
                &auth.0,
                task_id.0,
                QuerySubtasksFilter {
                    attempted: attempted.0,
                    solved: solved.0,
                    rated: rated.0,
                    enabled: enabled.0,
                    retired: retired.0,
                    creator: creator.0,
                    ty: None,
                },
                QuestionSummary::from,
            )
            .await?,
        )
    }

    /// Get a question by id.
    #[oai(path = "/tasks/:task_id/questions/:subtask_id", method = "get")]
    async fn get_question(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetQuestion::Response<VerifiedUserAuth> {
        match query_subtask::<challenges_questions::Entity, _>(
            &db,
            &auth.0,
            task_id.0,
            subtask_id.0,
            Question::from,
        )
        .await?
        {
            Some(mcq) => GetQuestion::ok(mcq),
            None => GetQuestion::subtask_not_found(),
        }
    }

    /// Get a question and its solution by id.
    #[oai(
        path = "/tasks/:task_id/questions/:subtask_id/solution",
        method = "get"
    )]
    async fn get_question_with_solution(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetQuestionWithSolution::Response<VerifiedUserAuth> {
        match query_subtask_admin::<challenges_questions::Entity, _>(
            &db,
            &auth.0,
            task_id.0,
            subtask_id.0,
            QuestionWithSolution::from,
        )
        .await?
        {
            Ok(matching) => GetQuestionWithSolution::ok(matching),
            Err(QuerySubtaskAdminError::NotFound) => GetQuestionWithSolution::subtask_not_found(),
            Err(QuerySubtaskAdminError::NoAccess) => GetQuestionWithSolution::forbidden(),
        }
    }

    /// Create a new question.
    #[oai(path = "/tasks/:task_id/questions", method = "post")]
    async fn create_question(
        &self,
        task_id: Path<Uuid>,
        data: Json<CreateQuestionRequest>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> CreateQuestion::Response<VerifiedUserAuth> {
        let subtask = match create_subtask(
            &db,
            &self.state.services,
            &self.config,
            &auth.0,
            task_id.0,
            data.0.subtask,
            ChallengesSubtaskType::Question,
        )
        .await?
        {
            Ok(subtask) => subtask,
            Err(CreateSubtaskError::TaskNotFound) => return CreateQuestion::task_not_found(),
            Err(CreateSubtaskError::Forbidden) => return CreateQuestion::forbidden(),
            Err(CreateSubtaskError::Banned(until)) => return CreateQuestion::banned(until),
            Err(CreateSubtaskError::XpLimitExceeded(x)) => {
                return CreateQuestion::xp_limit_exceeded(x)
            }
            Err(CreateSubtaskError::CoinLimitExceeded(x)) => {
                return CreateQuestion::coin_limit_exceeded(x)
            }
        };

        if !check_answers(
            &data.0.answers,
            data.0.ascii_letters,
            data.0.digits,
            data.0.punctuation,
        ) {
            return CreateQuestion::invalid_char();
        }

        let question = challenges_questions::ActiveModel {
            subtask_id: Set(subtask.id),
            question: Set(data.0.question),
            answers: Set(data.0.answers),
            case_sensitive: Set(data.0.case_sensitive),
            ascii_letters: Set(data.0.ascii_letters),
            digits: Set(data.0.digits),
            punctuation: Set(data.0.punctuation),
            blocks: Set(data.0.blocks),
        }
        .insert(&***db)
        .await?;
        CreateQuestion::ok(QuestionWithSolution::from(question, subtask))
    }

    /// Update a multiple choice question.
    #[oai(path = "/tasks/:task_id/questions/:subtask_id", method = "patch")]
    async fn update_question(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        data: Json<UpdateQuestionRequest>,
        db: Data<&DbTxn>,
        auth: AdminAuth,
    ) -> UpdateQuestion::Response<AdminAuth> {
        let (question, subtask) = match update_subtask::<challenges_questions::Entity>(
            &db,
            &auth.0,
            task_id.0,
            subtask_id.0,
            data.0.subtask,
        )
        .await?
        {
            Ok(x) => x,
            Err(UpdateSubtaskError::SubtaskNotFound) => return UpdateQuestion::subtask_not_found(),
            Err(UpdateSubtaskError::TaskNotFound) => return UpdateQuestion::task_not_found(),
        };

        if !check_answers(
            data.0.answers.get_new(&question.answers),
            *data.0.ascii_letters.get_new(&question.ascii_letters),
            *data.0.digits.get_new(&question.digits),
            *data.0.punctuation.get_new(&question.punctuation),
        ) {
            return UpdateQuestion::invalid_char();
        }

        let question = challenges_questions::ActiveModel {
            subtask_id: Unchanged(question.subtask_id),
            question: data.0.question.update(question.question),
            answers: data.0.answers.update(question.answers),
            case_sensitive: data.0.case_sensitive.update(question.case_sensitive),
            ascii_letters: data.0.ascii_letters.update(question.ascii_letters),
            digits: data.0.digits.update(question.digits),
            punctuation: data.0.punctuation.update(question.punctuation),
            blocks: data.0.blocks.update(question.blocks),
        }
        .update(&***db)
        .await?;

        UpdateQuestion::ok(QuestionWithSolution::from(question, subtask))
    }

    /// Attempt to solve a multiple choice question.
    #[oai(
        path = "/tasks/:task_id/questions/:subtask_id/attempts",
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
        let Some((question, subtask)) =
            get_subtask::<challenges_questions::Entity>(&db, task_id.0, subtask_id.0).await?
        else {
            return SolveQuestion::subtask_not_found();
        };
        if !auth.0.admin && auth.0.id != subtask.creator && !subtask.enabled {
            return SolveQuestion::subtask_not_found();
        }

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;

        let solved_previously = user_subtask.is_solved();
        if let Some(last_attempt) = user_subtask.last_attempt() {
            let time_left = self.config.challenges.questions.timeout as i64
                - (Utc::now() - last_attempt).num_seconds();
            if time_left > 0 {
                return SolveQuestion::too_many_requests(time_left as u64);
            }
        }

        if !deduct_hearts(&self.state.services, &self.config, &auth.0, &subtask).await? {
            return SolveQuestion::not_enough_hearts();
        }

        let answer = normalize_answer(&data.0.answer, question.case_sensitive);
        let solved = question
            .answers
            .iter()
            .any(|ans| normalize_answer(ans, question.case_sensitive) == answer);

        if !solved_previously {
            let now = Utc::now().naive_utc();
            if solved {
                update_user_subtask(
                    &db,
                    user_subtask.as_ref(),
                    challenges_user_subtasks::ActiveModel {
                        user_id: Set(auth.0.id),
                        subtask_id: Set(subtask.id),
                        solved_timestamp: Set(Some(now)),
                        last_attempt_timestamp: Set(Some(now)),
                        attempts: Set(user_subtask.attempts() as i32 + 1),
                        ..Default::default()
                    },
                )
                .await?;

                if auth.0.id != subtask.creator {
                    send_task_rewards(&self.state.services, &db, auth.0.id, &subtask).await?;
                }
            } else {
                update_user_subtask(
                    &db,
                    user_subtask.as_ref(),
                    challenges_user_subtasks::ActiveModel {
                        user_id: Set(auth.0.id),
                        subtask_id: Set(subtask.id),
                        last_attempt_timestamp: Set(Some(now)),
                        attempts: Set(user_subtask.attempts() as i32 + 1),
                        ..Default::default()
                    },
                )
                .await?;
            }
        }

        SolveQuestion::ok(SolveQuestionFeedback { solved })
    }
}

response!(ListQuestions = {
    Ok(200) => Vec<QuestionSummary>,
});

response!(GetQuestion = {
    Ok(200) => Question,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
});

response!(GetQuestionWithSolution = {
    Ok(200) => QuestionWithSolution,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user is not allowed to view the solution to this question.
    Forbidden(403, error),
});

response!(CreateQuestion = {
    Ok(201) => QuestionWithSolution,
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
    /// One of `ascii_letters`, `digits` or `punctuation` is set to `false`, but one of the `answers` contains such a character.
    InvalidChar(400, error),
});

response!(UpdateQuestion = {
    Ok(200) => QuestionWithSolution,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// Task does not exist.
    TaskNotFound(404, error),
    /// One of `ascii_letters`, `digits` or `punctuation` is set to `false`, but one of the `answers` contains such a character.
    InvalidChar(400, error),
});

response!(SolveQuestion = {
    Ok(201) => SolveQuestionFeedback,
    /// Try again later. `details` contains the number of seconds to wait.
    TooManyRequests(429, error) => u64,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user does not have enough hearts to submit a solution and is neither an admin nor the creator of this subtask.
    NotEnoughHearts(403, error),
});

fn check_answers(answers: &[String], ascii_letters: bool, digits: bool, punctuation: bool) -> bool {
    answers.iter().all(|answer| {
        answer.chars().all(|c| {
            (ascii_letters || !c.is_ascii_alphabetic())
                && (digits || !c.is_ascii_digit())
                && (punctuation || !c.is_ascii_punctuation())
        })
    })
}

fn normalize_answer(answer: &str, case_sensitive: bool) -> String {
    let answer = answer.trim();
    let mut out = String::with_capacity(answer.len());
    let mut whitespace = false;
    for c in answer.chars() {
        if c.is_whitespace() {
            if !whitespace {
                out.push(' ');
            }
            whitespace = true;
        } else {
            whitespace = false;
            out.push(if case_sensitive {
                c
            } else {
                c.to_ascii_lowercase()
            })
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_answer() {
        assert_eq!(normalize_answer("", true), "");
        assert_eq!(
            normalize_answer(" This     is my ANSWER!   \n\n \t  42 ", true),
            "This is my ANSWER! 42"
        );
        assert_eq!(
            normalize_answer(" This     is my ANSWER!   \n\n \t  42 ", false),
            "this is my answer! 42"
        );
    }
}
