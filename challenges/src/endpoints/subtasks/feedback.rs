use std::sync::Arc;

use chrono::Utc;
use entity::{
    challenges_user_subtasks,
    sea_orm_active_enums::{ChallengesRating, ChallengesReportReason, ChallengesSubtaskType},
};
use lib::{auth::VerifiedUserAuth, config::Config, SharedState};
use poem::web::Data;
use poem_ext::{db::DbTxn, response};
use poem_openapi::{param::Path, payload::Json, OpenApi};
use schemas::challenges::subtasks::PostFeedbackRequest;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

use super::{get_subtask, reports::create_report};
use crate::{
    endpoints::Tags,
    services::subtasks::{get_user_subtask, update_user_subtask, UserSubtaskExt},
};

pub struct Api {
    pub state: Arc<SharedState>,
    pub config: Arc<Config>,
}

#[OpenApi(tag = "Tags::Subtasks")]
impl Api {
    /// Submit feedback for a subtask after solving it.
    #[oai(
        path = "/tasks/:task_id/subtasks/:subtask_id/feedback",
        method = "post"
    )]
    pub async fn post_feedback(
        &self,
        task_id: Path<Uuid>,
        subtask_id: Path<Uuid>,
        data: Json<PostFeedbackRequest>,
        db: Data<&DbTxn>,
        auth: VerifiedUserAuth,
    ) -> PostFeedback::Response<VerifiedUserAuth> {
        let Some((subtask, _)) = get_subtask(&db, task_id.0, subtask_id.0).await? else {
            return PostFeedback::subtask_not_found();
        };
        if !auth.0.admin && auth.0.id != subtask.creator && !subtask.enabled {
            return PostFeedback::subtask_not_found();
        }

        let user_subtask = get_user_subtask(&db, auth.0.id, subtask.id).await?;
        if !user_subtask.can_rate(&auth.0, &subtask) {
            return PostFeedback::permission_denied();
        }

        update_user_subtask(
            &db,
            user_subtask.as_ref(),
            challenges_user_subtasks::ActiveModel {
                user_id: Set(auth.0.id),
                subtask_id: Set(subtask.id),
                rating: Set(Some(data.0.rating)),
                rating_timestamp: Set(Some(Utc::now().naive_utc())),
                ..Default::default()
            },
        )
        .await?;

        if data.0.rating == ChallengesRating::Positive {
            let config = &self.config.challenges;
            let coins = match subtask.ty {
                ChallengesSubtaskType::CodingChallenge => config.coding_challenges.creator_coins,
                ChallengesSubtaskType::Matching => config.matchings.creator_coins,
                ChallengesSubtaskType::MultipleChoiceQuestion => {
                    config.multiple_choice_questions.creator_coins
                }
                ChallengesSubtaskType::Question => config.questions.creator_coins,
            };
            self.state
                .services
                .shop
                .add_coins(subtask.creator, coins as _, "Quiz/Challenge", true)
                .await??;
        }

        if data.0.rating == ChallengesRating::Negative {
            let ratings = challenges_user_subtasks::Entity::find()
                .filter(challenges_user_subtasks::Column::UserId.eq(subtask.id))
                .filter(challenges_user_subtasks::Column::Rating.is_not_null())
                .all(&***db)
                .await?;
            let positive = ratings
                .iter()
                .filter(|x| x.rating == Some(ChallengesRating::Positive))
                .count();
            let negative = ratings
                .iter()
                .filter(|x| x.rating == Some(ChallengesRating::Negative))
                .count();
            if negative >= 10 && negative > positive {
                create_report(
                    &db,
                    None,
                    subtask,
                    None,
                    ChallengesReportReason::Dislike,
                    format!(
                        "Subtask has received more dislikes ({negative}) than likes ({positive})."
                    ),
                )
                .await?;
            }
        }

        PostFeedback::created()
    }
}

response!(PostFeedback = {
    Created(201),
    /// The subtask does not exist.
    SubtaskNotFound(404, error),
    /// The user is not allowed to post feeback for this subtask.
    PermissionDenied(403, error),
});
