use std::{collections::HashSet, sync::Arc};

use chrono::{DateTime, Utc};
use entity::{
    challenges_matching_attempts, challenges_matchings, challenges_user_subtasks,
    sea_orm_active_enums::ChallengesSubtaskType,
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
use schemas::challenges::matchings::{
    CreateMatchingRequest, Matching, MatchingSummary, MatchingWithSolution, SolveMatchingFeedback,
    SolveMatchingRequest, UpdateMatchingRequest,
};
use sea_orm::{ActiveModelTrait, ColumnTrait, ModelTrait, QueryFilter, QueryOrder, Set, Unchanged};
use uuid::Uuid;

use super::Tags;
use crate::services::subtasks::{
    create_subtask, get_subtask, get_user_subtask, query_subtask, query_subtask_admin,
    query_subtasks, send_task_rewards, update_subtask, update_user_subtask, CreateSubtaskError,
    QuerySubtaskError, QuerySubtasksFilter, UpdateSubtaskError, UserSubtaskExt,
};

pub struct Matchings {
    pub state: Arc<SharedState>,
    pub config: Arc<Config>,
}

#[OpenApi(tag = "Tags::Matchings")]
impl Matchings {
    /// List all matchings in a task.
    #[oai(path = "/tasks/:task_id/matchings", method = "get")]
    #[allow(clippy::too_many_arguments)]
    async fn list_matchings(
        &self,
        task_id: Path<Uuid>,
        /// Whether to search for free matchings.
        free: Query<Option<bool>>,
        /// Whether to search for unlocked matchings.
        unlocked: Query<Option<bool>>,
        /// Whether to search for solved matchings.
        solved: Query<Option<bool>>,
        /// Whether to search for rated matchings.
        rated: Query<Option<bool>>,
        /// Whether to search for enabled subtasks.
        enabled: Query<Option<bool>>,
        /// Filter by creator.
        creator: Query<Option<Uuid>>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> ListMatchings::Response<VerifiedUserAuth> {
        ListMatchings::ok(
            query_subtasks::<challenges_matchings::Entity, _>(
                &db,
                &auth.0,
                task_id.0,
                QuerySubtasksFilter {
                    free: free.0,
                    unlocked: unlocked.0,
                    solved: solved.0,
                    rated: rated.0,
                    enabled: enabled.0,
                    creator: creator.0,
                },
                MatchingSummary::from,
            )
            .await?,
        )
    }

    /// Get a matching by id.
    #[oai(path = "/tasks/:task_id/matchings/:subtask_id", method = "get")]
    async fn get_matching(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetMatching::Response<VerifiedUserAuth> {
        match query_subtask::<challenges_matchings::Entity, _>(
            &db,
            &auth.0,
            task_id.0,
            subtask_id.0,
            Matching::from,
        )
        .await?
        {
            Ok(matching) => GetMatching::ok(matching),
            Err(QuerySubtaskError::NotFound) => GetMatching::subtask_not_found(),
            Err(QuerySubtaskError::NoAccess) => GetMatching::no_access(),
        }
    }

    /// Get a matching and its solution by id.
    #[oai(
        path = "/tasks/:task_id/matchings/:subtask_id/solution",
        method = "get"
    )]
    async fn get_matching_with_solution(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> GetMatchingWithSolution::Response<VerifiedUserAuth> {
        match query_subtask_admin::<challenges_matchings::Entity, _>(
            &db,
            &auth.0,
            task_id.0,
            subtask_id.0,
            MatchingWithSolution::from,
        )
        .await?
        {
            Ok(matching) => GetMatchingWithSolution::ok(matching),
            Err(QuerySubtaskError::NotFound) => GetMatchingWithSolution::subtask_not_found(),
            Err(QuerySubtaskError::NoAccess) => GetMatchingWithSolution::forbidden(),
        }
    }

    /// Create a new matching.
    #[oai(path = "/tasks/:task_id/matchings", method = "post")]
    async fn create_matching(
        &self,
        task_id: Path<Uuid>,
        data: Json<CreateMatchingRequest>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> CreateMatching::Response<VerifiedUserAuth> {
        let subtask = match create_subtask(
            &db,
            &self.state.services,
            &self.config,
            &auth.0,
            task_id.0,
            data.0.subtask,
            ChallengesSubtaskType::Matching,
        )
        .await?
        {
            Ok(subtask) => subtask,
            Err(CreateSubtaskError::TaskNotFound) => return CreateMatching::task_not_found(),
            Err(CreateSubtaskError::Forbidden) => return CreateMatching::forbidden(),
            Err(CreateSubtaskError::Banned(until)) => return CreateMatching::banned(until),
            Err(CreateSubtaskError::XpLimitExceeded(x)) => {
                return CreateMatching::xp_limit_exceeded(x)
            }
            Err(CreateSubtaskError::CoinLimitExceeded(x)) => {
                return CreateMatching::coin_limit_exceeded(x)
            }
            Err(CreateSubtaskError::FeeLimitExceeded(x)) => {
                return CreateMatching::fee_limit_exceeded(x)
            }
        };

        match check_matching(&data.0.left, &data.0.right, &data.0.solution) {
            Ok(()) => {}
            Err(InvalidMatchingError::LeftRightDifferentLength) => {
                return CreateMatching::left_right_different_length()
            }
            Err(InvalidMatchingError::SolutionDifferentLength) => {
                return CreateMatching::solution_different_length()
            }
            Err(InvalidMatchingError::InvalidIndex(x)) => return CreateMatching::invalid_index(x),
            Err(InvalidMatchingError::RightEntriesNotMatched(x)) => {
                return CreateMatching::right_entries_not_matched(x)
            }
        }

        let matching = challenges_matchings::ActiveModel {
            subtask_id: Set(subtask.id),
            left: Set(data.0.left),
            right: Set(data.0.right),
            solution: Set(data.0.solution.into_iter().map(|x| x as _).collect()),
        }
        .insert(&***db)
        .await?;
        CreateMatching::ok(MatchingWithSolution::from(matching, subtask))
    }

    /// Update a multiple choice matching.
    #[oai(path = "/tasks/:task_id/matchings/:subtask_id", method = "patch")]
    async fn update_matching(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        data: Json<UpdateMatchingRequest>,
        db: Data<&DbTxn>,
        auth: AdminAuth,
    ) -> UpdateMatching::Response<AdminAuth> {
        let (matching, subtask) = match update_subtask::<challenges_matchings::Entity>(
            &db,
            &auth.0,
            task_id.0,
            subtask_id.0,
            data.0.subtask,
        )
        .await?
        {
            Ok(x) => x,
            Err(UpdateSubtaskError::SubtaskNotFound) => return UpdateMatching::subtask_not_found(),
            Err(UpdateSubtaskError::TaskNotFound) => return UpdateMatching::task_not_found(),
        };

        match check_matching(
            data.0.left.get_new(&matching.left),
            data.0.right.get_new(&matching.right),
            data.0
                .solution
                .get_new(&matching.solution.iter().map(|&x| x as _).collect()),
        ) {
            Ok(()) => {}
            Err(InvalidMatchingError::LeftRightDifferentLength) => {
                return UpdateMatching::left_right_different_length()
            }
            Err(InvalidMatchingError::SolutionDifferentLength) => {
                return UpdateMatching::solution_different_length()
            }
            Err(InvalidMatchingError::InvalidIndex(x)) => return UpdateMatching::invalid_index(x),
            Err(InvalidMatchingError::RightEntriesNotMatched(x)) => {
                return UpdateMatching::right_entries_not_matched(x)
            }
        }

        let matching = challenges_matchings::ActiveModel {
            subtask_id: Unchanged(matching.subtask_id),
            left: data.0.left.update(matching.left),
            right: data.0.right.update(matching.right),
            solution: data
                .0
                .solution
                .map(|x| x.into_iter().map(|x| x as _).collect())
                .update(matching.solution),
        }
        .update(&***db)
        .await?;

        UpdateMatching::ok(MatchingWithSolution::from(matching, subtask))
    }

    /// Attempt to solve a multiple choice matching.
    #[oai(
        path = "/tasks/:task_id/matchings/:subtask_id/attempts",
        method = "post"
    )]
    async fn solve_matching(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        data: Json<SolveMatchingRequest>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> SolveMatching::Response<VerifiedUserAuth> {
        let Some((matching, subtask)) = get_subtask::<challenges_matchings::Entity>(&db, task_id.0, subtask_id.0).await? else {
                return SolveMatching::subtask_not_found();
            };
        if !auth.0.admin && auth.0.id != subtask.creator && !subtask.enabled {
            return SolveMatching::subtask_not_found();
        }

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;
        if !user_subtask.check_access(&auth.0, &subtask) {
            return SolveMatching::no_access();
        }

        if data.0.answer.len() != matching.solution.len() {
            return SolveMatching::solution_different_length();
        }

        let previous_attempts = matching
            .find_related(challenges_matching_attempts::Entity)
            .filter(challenges_matching_attempts::Column::UserId.eq(auth.0.id))
            .order_by_desc(challenges_matching_attempts::Column::Timestamp)
            .all(&***db)
            .await?;
        let solved_previously = user_subtask.is_solved();
        if let Some(last_attempt) = previous_attempts.first() {
            let time_left = self.config.challenges.matchings.timeout_incr as i64
                * previous_attempts.len() as i64
                - (Utc::now().naive_utc() - last_attempt.timestamp).num_seconds();
            if !solved_previously && time_left > 0 {
                return SolveMatching::too_many_requests(time_left as u64);
            }
        }

        let correct = data
            .0
            .answer
            .iter()
            .zip(matching.solution.iter())
            .filter(|(&x, &y)| x == y as u8)
            .count();
        let solved = correct == matching.solution.len();

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

            challenges_matching_attempts::ActiveModel {
                id: Set(Uuid::new_v4()),
                matching_id: Set(matching.subtask_id),
                user_id: Set(auth.0.id),
                timestamp: Set(now),
                solved: Set(solved),
            }
            .insert(&***db)
            .await?;
        }

        SolveMatching::ok(SolveMatchingFeedback { solved, correct })
    }
}

response!(ListMatchings = {
    Ok(200) => Vec<MatchingSummary>,
});

response!(GetMatching = {
    Ok(200) => Matching,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user has not unlocked this matching.
    NoAccess(403, error),
});

response!(GetMatchingWithSolution = {
    Ok(200) => MatchingWithSolution,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user is not allowed to view the solution to this matching.
    Forbidden(403, error),
});

response!(CreateMatching = {
    Ok(201) => MatchingWithSolution,
    /// Task does not exist.
    TaskNotFound(404, error),
    /// The user is not allowed to create matchings in this task.
    Forbidden(403, error),
    /// The user is currently banned from creating subtasks.
    Banned(403, error) => Option<DateTime<Utc>>,
    /// The max xp limit has been exceeded.
    XpLimitExceeded(403, error) => u64,
    /// The max coin limit has been exceeded.
    CoinLimitExceeded(403, error) => u64,
    /// The max fee limit has been exceeded.
    FeeLimitExceeded(403, error) => u64,
    /// The left list does not contain the same number of entries as the right list.
    LeftRightDifferentLength(400, error),
    /// The solution list does not contain the same number of entries as the left and right lists.
    SolutionDifferentLength(400, error),
    /// The solution list contains an invalid index.
    InvalidIndex(400, error) => u8,
    /// One or more entries in the right list have no match in the left list.
    RightEntriesNotMatched(400, error) => HashSet<u8>,
});

response!(UpdateMatching = {
    Ok(200) => MatchingWithSolution,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// Task does not exist.
    TaskNotFound(404, error),
    /// The left list does not contain the same number of entries as the right list.
    LeftRightDifferentLength(400, error),
    /// The solution list does not contain the same number of entries as the left and right lists.
    SolutionDifferentLength(400, error),
    /// The solution list contains an invalid index.
    InvalidIndex(400, error) => u8,
    /// One or more entries in the right list have no match in the left list.
    RightEntriesNotMatched(400, error) => HashSet<u8>,
});

response!(SolveMatching = {
    Ok(201) => SolveMatchingFeedback,
    /// Try again later. `details` contains the number of seconds to wait.
    TooManyRequests(429, error) => u64,
    /// Subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user has not unlocked this matching.
    NoAccess(403, error),
    /// The solution list does not contain the same number of entries as the left and right lists.
    SolutionDifferentLength(400, error),
});

fn check_matching(
    left: &[String],
    right: &[String],
    solution: &[u8],
) -> Result<(), InvalidMatchingError> {
    let n = left.len();
    if right.len() != n {
        return Err(InvalidMatchingError::LeftRightDifferentLength);
    }
    if solution.len() != n {
        return Err(InvalidMatchingError::SolutionDifferentLength);
    }
    if let Some(&x) = solution.iter().find(|&&x| x >= n as _) {
        return Err(InvalidMatchingError::InvalidIndex(x));
    }
    let mut not_matched: HashSet<u8> = (0..n as _).collect();
    for &x in solution {
        not_matched.remove(&x);
    }
    if !not_matched.is_empty() {
        return Err(InvalidMatchingError::RightEntriesNotMatched(not_matched));
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum InvalidMatchingError {
    LeftRightDifferentLength,
    SolutionDifferentLength,
    InvalidIndex(u8),
    RightEntriesNotMatched(HashSet<u8>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_matching() {
        let left = ["A".into(), "B".into(), "C".into()];
        let right = ["X".into(), "Y".into(), "Z".into()];
        let solution = [2, 0, 1];
        assert_eq!(check_matching(&left, &right, &solution), Ok(()));
        assert_eq!(
            check_matching(&left, &right, &[2, 0, 1, 3]),
            Err(InvalidMatchingError::SolutionDifferentLength)
        );
        assert_eq!(
            check_matching(&left, &right, &[2, 0, 3]),
            Err(InvalidMatchingError::InvalidIndex(3))
        );
        assert_eq!(
            check_matching(&left, &right, &[2, 0, 2]),
            Err(InvalidMatchingError::RightEntriesNotMatched([1].into()))
        );
        assert_eq!(
            check_matching(&left, &right, &[1, 1, 1]),
            Err(InvalidMatchingError::RightEntriesNotMatched([0, 2].into()))
        );
        assert_eq!(
            check_matching(&left, &["foo".into()], &solution),
            Err(InvalidMatchingError::LeftRightDifferentLength)
        );
    }
}
