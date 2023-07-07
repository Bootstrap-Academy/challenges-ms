use std::sync::Arc;

use chrono::{DateTime, Utc};
use entity::{
    challenges_question_attempts, challenges_questions, challenges_subtasks,
    challenges_user_subtasks, sea_orm_active_enums::ChallengesBanAction,
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
use sea_orm::{ActiveModelTrait, ColumnTrait, ModelTrait, QueryFilter, QueryOrder, Set, Unchanged};
use uuid::Uuid;

use super::Tags;
use crate::{
    schemas::{
        question::{
            CreateQuestionRequest, Question, QuestionSummary, QuestionWithSolution,
            SolveQuestionFeedback, SolveQuestionRequest, UpdateQuestionRequest,
        },
        subtasks::Subtask,
    },
    services::{
        subtasks::{
            can_create, get_active_ban, get_subtask, get_user_subtask, query_subtasks,
            send_task_rewards, update_user_subtask, ActiveBan, QuerySubtasksFilter, UserSubtaskExt,
        },
        tasks::{get_task, get_task_with_specific, Task},
    },
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
    ) -> ListQuestions::Response<VerifiedUserAuth> {
        ListQuestions::ok(
            query_subtasks::<challenges_questions::Entity, _>(
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
        let Some((question, subtask)) = get_subtask::<challenges_questions::Entity>(&db, task_id.0, subtask_id.0).await? else {
            return GetQuestion::subtask_not_found();
        };
        if !auth.0.admin && auth.0.id != subtask.creator && !subtask.enabled {
            return GetQuestion::subtask_not_found();
        }

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;
        if !user_subtask.check_access(&auth.0, &subtask) {
            return GetQuestion::no_access();
        }

        GetQuestion::ok(Question::from(
            question,
            Subtask::from(
                subtask,
                true,
                user_subtask.is_solved(),
                user_subtask.is_rated(),
            ),
        ))
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
        let Some((question, subtask)) = get_subtask::<challenges_questions::Entity>(&db, task_id.0, subtask_id.0).await? else {
            return GetQuestionWithSolution::subtask_not_found();
        };

        if !(auth.0.admin || auth.0.id == subtask.creator) {
            return GetQuestionWithSolution::forbidden();
        }

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;
        GetQuestionWithSolution::ok(QuestionWithSolution::from(
            question,
            Subtask::from(
                subtask,
                true,
                user_subtask.is_solved(),
                user_subtask.is_rated(),
            ),
        ))
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
        let (task, specific) = match get_task_with_specific(&db, task_id.0).await? {
            Some(task) => task,
            None => return CreateQuestion::task_not_found(),
        };
        if !can_create(&self.state.services, &self.config, &specific, &auth.0).await? {
            return CreateQuestion::forbidden();
        }

        let xp = data
            .0
            .subtask
            .xp
            .unwrap_or(self.config.challenges.quizzes.max_xp);
        let coins = data
            .0
            .subtask
            .coins
            .unwrap_or(self.config.challenges.quizzes.max_coins);
        if matches!(specific, Task::CourseTask(_)) && !auth.0.admin {
            if xp > self.config.challenges.quizzes.max_xp {
                return CreateQuestion::xp_limit_exceeded(self.config.challenges.quizzes.max_xp);
            }
            if coins > self.config.challenges.quizzes.max_coins {
                return CreateQuestion::coin_limit_exceeded(
                    self.config.challenges.quizzes.max_coins,
                );
            }
            if data.0.subtask.fee > self.config.challenges.quizzes.max_fee {
                return CreateQuestion::fee_limit_exceeded(self.config.challenges.quizzes.max_fee);
            }
        }

        match get_active_ban(&db, &auth.0, ChallengesBanAction::Create).await? {
            ActiveBan::NotBanned => {}
            ActiveBan::Temporary(end) => return CreateQuestion::banned(Some(end)),
            ActiveBan::Permanent => return CreateQuestion::banned(None),
        }

        if !check_answers(
            &data.0.answers,
            data.0.ascii_letters,
            data.0.digits,
            data.0.punctuation,
        ) {
            return CreateQuestion::invalid_char();
        }

        let subtask = challenges_subtasks::ActiveModel {
            id: Set(Uuid::new_v4()),
            task_id: Set(task.id),
            creator: Set(auth.0.id),
            creation_timestamp: Set(Utc::now().naive_utc()),
            xp: Set(xp as _),
            coins: Set(coins as _),
            fee: Set(data.0.subtask.fee as _),
            enabled: Set(true),
        }
        .insert(&***db)
        .await?;
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
        CreateQuestion::ok(QuestionWithSolution::from(
            question,
            Subtask::from(subtask, true, false, false),
        ))
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
        let Some((question, subtask)) = get_subtask::<challenges_questions::Entity>(&db, task_id.0, subtask_id.0).await? else {
            return UpdateQuestion::subtask_not_found();
        };

        if get_task(&db, *data.0.subtask.task_id.get_new(&subtask.task_id))
            .await?
            .is_none()
        {
            return UpdateQuestion::task_not_found();
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
        let subtask = challenges_subtasks::ActiveModel {
            id: Unchanged(subtask.id),
            task_id: data.0.subtask.task_id.update(subtask.task_id),
            creator: Unchanged(subtask.creator),
            creation_timestamp: Unchanged(subtask.creation_timestamp),
            xp: data.0.subtask.xp.map(|x| x as _).update(subtask.xp),
            coins: data.0.subtask.coins.map(|x| x as _).update(subtask.coins),
            fee: data.0.subtask.fee.map(|x| x as _).update(subtask.fee),
            enabled: data.0.subtask.enabled.update(subtask.enabled),
        }
        .update(&***db)
        .await?;

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;
        UpdateQuestion::ok(QuestionWithSolution::from(
            question,
            Subtask::from(
                subtask,
                true,
                user_subtask.is_solved(),
                user_subtask.is_rated(),
            ),
        ))
    }

    /// Delete a multiple choice question.
    #[oai(path = "/tasks/:task_id/questions/:subtask_id", method = "delete")]
    async fn delete_question(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        _auth: AdminAuth,
    ) -> DeleteQuestion::Response<AdminAuth> {
        match get_subtask::<challenges_questions::Entity>(&db, task_id.0, subtask_id.0).await? {
            Some((_, subtask)) => {
                subtask.delete(&***db).await?;
                DeleteQuestion::ok()
            }
            None => DeleteQuestion::subtask_not_found(),
        }
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
        let Some((question, subtask)) = get_subtask::<challenges_questions::Entity>(&db, task_id.0, subtask_id.0).await? else {
                return SolveQuestion::subtask_not_found();
            };
        if !auth.0.admin && auth.0.id != subtask.creator && !subtask.enabled {
            return SolveQuestion::subtask_not_found();
        }

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;
        if !user_subtask.check_access(&auth.0, &subtask) {
            return SolveQuestion::no_access();
        }

        let previous_attempts = question
            .find_related(challenges_question_attempts::Entity)
            .filter(challenges_question_attempts::Column::UserId.eq(auth.0.id))
            .order_by_desc(challenges_question_attempts::Column::Timestamp)
            .all(&***db)
            .await?;
        let solved_previously = user_subtask.is_solved();
        if let Some(last_attempt) = previous_attempts.first() {
            let time_left = self.config.challenges.questions.timeout_incr as i64
                * previous_attempts.len() as i64
                - (Utc::now().naive_utc() - last_attempt.timestamp).num_seconds();
            if !solved_previously && time_left > 0 {
                return SolveQuestion::too_many_requests(time_left as u64);
            }
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

            challenges_question_attempts::ActiveModel {
                id: Set(Uuid::new_v4()),
                question_id: Set(question.subtask_id),
                user_id: Set(auth.0.id),
                timestamp: Set(now),
                solved: Set(solved),
            }
            .insert(&***db)
            .await?;
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
    /// The user has not unlocked this question.
    NoAccess(403, error),
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
    /// The max fee limit has been exceeded.
    FeeLimitExceeded(403, error) => u64,
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

response!(DeleteQuestion = {
    Ok(200),
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
});

response!(SolveQuestion = {
    Ok(201) => SolveQuestionFeedback,
    /// Try again later. `details` contains the number of seconds to wait.
    TooManyRequests(429, error) => u64,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user has not unlocked this question.
    NoAccess(403, error),
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
