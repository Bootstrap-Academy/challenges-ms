use std::sync::Arc;

use chrono::{DateTime, Utc};
use entity::{
    challenges_multiple_choice_attempts, challenges_multiple_choice_quizes, challenges_subtasks,
    challenges_user_subtasks, sea_orm_active_enums::ChallengesBanAction,
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
        MultipleChoiceQuestion, MultipleChoiceQuestionSummary, SolveMCQFeedback, SolveMCQRequest,
        UpdateMultipleChoiceQuestionRequest,
    },
    services::{
        subtasks::{
            can_create, get_active_ban, get_user_subtask, get_user_subtasks, send_task_rewards,
            update_user_subtask, ActiveBan, UserSubtaskExt,
        },
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
        let subtasks = get_user_subtasks(&db, auth.0.id).await?;
        ListMCQs::ok(
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
                    let unlocked_ = subtasks.get(&id).check_access(&auth.0, &subtask);
                    let solved_ = subtasks.get(&id).is_solved();
                    let rated_ = subtasks.get(&id).is_rated();
                    let enabled_ = subtask.enabled;
                    ((auth.0.admin || auth.0.id == subtask.creator || subtask.enabled)
                        && free.unwrap_or(free_) == free_
                        && unlocked.unwrap_or(unlocked_) == unlocked_
                        && solved.unwrap_or(solved_) == solved_
                        && rated.unwrap_or(rated_) == rated_
                        && enabled.unwrap_or(enabled_) == enabled_)
                        .then_some(MultipleChoiceQuestionSummary::from(
                            mcq, subtask, unlocked_, solved_, rated_,
                        ))
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
    ) -> GetMCQ::Response<VerifiedUserAuth> {
        let Some((mcq, subtask)) = get_question(&db, task_id.0, subtask_id.0).await? else {
            return GetMCQ::subtask_not_found();
        };
        if !auth.0.admin && auth.0.id != subtask.creator && !subtask.enabled {
            return GetMCQ::subtask_not_found();
        }

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;
        if !user_subtask.check_access(&auth.0, &subtask) {
            return GetMCQ::no_access();
        }

        GetMCQ::ok(MultipleChoiceQuestion::<String>::from(
            mcq,
            subtask,
            user_subtask.is_solved(),
            user_subtask.is_rated(),
        ))
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
        let Some((mcq, subtask)) = get_question(&db, task_id.0, subtask_id.0).await? else {
            return GetMCQWithSolution::subtask_not_found();
        };

        if !(auth.0.admin || auth.0.id == subtask.creator) {
            return GetMCQWithSolution::forbidden();
        }

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;
        GetMCQWithSolution::ok(MultipleChoiceQuestion::<Answer>::from(
            mcq,
            subtask,
            user_subtask.is_solved(),
            user_subtask.is_rated(),
        ))
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
        let (task, specific) = match get_task_with_specific(&db, task_id.0).await? {
            Some(task) => task,
            None => return CreateMCQ::task_not_found(),
        };
        if !can_create(&self.state.services, &self.config, &specific, &auth.0).await? {
            return CreateMCQ::forbidden();
        }

        let xp = data.0.xp.unwrap_or(self.config.challenges.quizzes.max_xp);
        let coins = data
            .0
            .coins
            .unwrap_or(self.config.challenges.quizzes.max_coins);
        if matches!(specific, Task::CourseTask(_)) && !auth.0.admin {
            if xp > self.config.challenges.quizzes.max_xp {
                return CreateMCQ::xp_limit_exceeded(self.config.challenges.quizzes.max_xp);
            }
            if coins > self.config.challenges.quizzes.max_coins {
                return CreateMCQ::coin_limit_exceeded(self.config.challenges.quizzes.max_coins);
            }
            if data.0.fee > self.config.challenges.quizzes.max_fee {
                return CreateMCQ::fee_limit_exceeded(self.config.challenges.quizzes.max_fee);
            }
        }

        match get_active_ban(&db, &auth.0, ChallengesBanAction::Create).await? {
            ActiveBan::NotBanned => {}
            ActiveBan::Temporary(end) => return CreateMCQ::banned(Some(end)),
            ActiveBan::Permanent => return CreateMCQ::banned(None),
        }

        let correct_cnt = data.0.answers.iter().filter(|x| x.correct).count();
        if data.0.single_choice && correct_cnt != 1 {
            return CreateMCQ::invalid_single_choice();
        }
        if correct_cnt == 0 {
            return CreateMCQ::invalid_multiple_choice();
        }

        let subtask = challenges_subtasks::ActiveModel {
            id: Set(Uuid::new_v4()),
            task_id: Set(task.id),
            creator: Set(auth.0.id),
            creation_timestamp: Set(Utc::now().naive_utc()),
            xp: Set(xp as _),
            coins: Set(coins as _),
            fee: Set(data.0.fee as _),
            enabled: Set(true),
        }
        .insert(&***db)
        .await?;
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
        CreateMCQ::ok(MultipleChoiceQuestion::<Answer>::from(
            mcq, subtask, false, false,
        ))
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
        let Some((mcq, subtask)) = get_question(&db, task_id.0, subtask_id.0).await? else {
            return UpdateMCQ::subtask_not_found();
        };

        if get_task(&db, *data.0.task_id.get_new(&subtask.task_id))
            .await?
            .is_none()
        {
            return UpdateMCQ::task_not_found();
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
        let subtask = challenges_subtasks::ActiveModel {
            id: Unchanged(subtask.id),
            task_id: data.0.task_id.update(subtask.task_id),
            creator: Unchanged(subtask.creator),
            creation_timestamp: Unchanged(subtask.creation_timestamp),
            xp: data.0.xp.map(|x| x as _).update(subtask.xp),
            coins: data.0.coins.map(|x| x as _).update(subtask.coins),
            fee: data.0.fee.map(|x| x as _).update(subtask.fee),
            enabled: data.0.enabled.update(subtask.enabled),
        }
        .update(&***db)
        .await?;

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;
        UpdateMCQ::ok(MultipleChoiceQuestion::<Answer>::from(
            mcq,
            subtask,
            user_subtask.is_solved(),
            user_subtask.is_rated(),
        ))
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
    ) -> DeleteMCQ::Response<AdminAuth> {
        match get_question(&db, task_id.0, subtask_id.0).await? {
            Some((_, subtask)) => {
                subtask.delete(&***db).await?;
                DeleteMCQ::ok()
            }
            None => DeleteMCQ::subtask_not_found(),
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
        data: Json<SolveMCQRequest>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> SolveMCQ::Response<VerifiedUserAuth> {
        let Some((mcq, subtask)) = get_question(&db, task_id.0, subtask_id.0).await? else {
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

response!(DeleteMCQ = {
    Ok(200),
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
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
