use std::sync::Arc;

use chrono::Utc;
use entity::{
    challenges_user_subtasks,
    sea_orm_active_enums::{ChallengesRating, ChallengesReportReason},
};
use lib::{auth::VerifiedUserAuth, SharedState};
use poem::web::Data;
use poem_ext::{db::DbTxn, response};
use poem_openapi::{param::Path, payload::Json, OpenApi};
use schemas::challenges::subtasks::PostFeedbackRequest;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Select, Set};
use uuid::Uuid;

use super::{get_subtask, reports::create_report};
use crate::{
    endpoints::Tags,
    services::subtasks::{get_user_subtask, update_user_subtask, UserSubtaskExt},
};

pub struct Api {
    pub state: Arc<SharedState>,
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
        crate::services::moderation::lock_subtask(&db, subtask_id.0).await?;
        let Some((subtask, _)) = get_subtask(&db, task_id.0, subtask_id.0).await? else {
            return PostFeedback::subtask_not_found();
        };
        if !auth.0.admin
            && (subtask.moderation_removed || (auth.0.id != subtask.creator && !subtask.enabled))
        {
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

        // Ratings remain useful feedback. New ratings never mint MorphCoins;
        // past financial records and already committed rewards stay intact.

        if data.0.rating == ChallengesRating::Negative {
            let ratings = subtask_ratings(subtask.id).all(&***db).await?;
            let (positive, negative) = count_ratings(ratings.iter().map(|x| x.rating));
            // A dismissed rating cohort must not be replayed as a fresh restriction.
            // A changed cohort is retained as a genuinely new notice for review.
            let prior = crate::services::moderation::value(&db,
                "SELECT to_jsonb(EXISTS(SELECT 1 FROM moderation_cases WHERE target_kind='subtask' AND target_id=$1 AND source='rating_threshold' AND private_evidence->'rating_counts'->>'cohort'=(SELECT md5(coalesce(jsonb_agg(jsonb_build_array(user_id,rating) ORDER BY user_id) FILTER(WHERE rating IS NOT NULL),'[]')::text) FROM challenges_user_subtasks WHERE subtask_id=$1) AND coalesce((private_evidence->>'content_revision')::bigint,0)=coalesce((SELECT content_revision FROM moderation_targets WHERE kind='subtask' AND id=$1),0))) AS value",
                vec![subtask.id.into()]).await?;
            if should_auto_hide(positive, negative) && prior == serde_json::Value::Bool(false) {
                let basis=self.state.services.auth.moderation_basis(subtask.creator).await.unwrap_or_else(|_|serde_json::json!({"automatic_quality_basis_confirmed":false,"recorded_acceptance":"unavailable"}));
                create_report(
                    &db,
                    None,
                    Uuid::new_v4(),
                    subtask,
                    None,
                    ChallengesReportReason::Dislike,
                    format!(
                        "{negative} negative und {positive} positive Bewertungen (mindestens 10 negative und mehr negative als positive Bewertungen)."
                    ),
                    basis,
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

/// Number of negative ratings from which a subtask is hidden automatically.
const AUTO_HIDE_NEGATIVE_RATINGS: usize = 10;

/// All ratings a subtask has received.
fn subtask_ratings(subtask_id: Uuid) -> Select<challenges_user_subtasks::Entity> {
    challenges_user_subtasks::Entity::find()
        .filter(challenges_user_subtasks::Column::SubtaskId.eq(subtask_id))
        .filter(challenges_user_subtasks::Column::Rating.is_not_null())
}

/// Count the positive and the negative ratings, in that order.
fn count_ratings(ratings: impl IntoIterator<Item = Option<ChallengesRating>>) -> (usize, usize) {
    let mut positive = 0;
    let mut negative = 0;
    for rating in ratings {
        match rating {
            Some(ChallengesRating::Positive) => positive += 1,
            Some(ChallengesRating::Negative) => negative += 1,
            Some(ChallengesRating::Neutral) | None => {}
        }
    }
    (positive, negative)
}

/// Whether a subtask with these ratings is hidden and reported for review.
fn should_auto_hide(positive: usize, negative: usize) -> bool {
    negative >= AUTO_HIDE_NEGATIVE_RATINGS && negative > positive
}

#[cfg(test)]
mod tests {
    use sea_orm::{DbBackend, QueryTrait};

    use super::*;

    fn ratings(positive: usize, negative: usize) -> Vec<Option<ChallengesRating>> {
        let mut ratings = vec![Some(ChallengesRating::Positive); positive];
        ratings.resize(positive + negative, Some(ChallengesRating::Negative));
        ratings
    }

    /// The ratings of a subtask are the ones given *for* it, not the ones
    /// given *by* the user whose id happens to match.
    #[test]
    fn ratings_are_looked_up_by_subtask() {
        let sql = subtask_ratings(Uuid::nil())
            .build(DbBackend::Postgres)
            .to_string();

        assert!(sql.contains(r#""subtask_id" ="#), "{sql}");
        assert!(!sql.contains(r#""user_id" ="#), "{sql}");
    }

    #[test]
    fn only_positive_and_negative_ratings_are_counted() {
        let mixed = [
            Some(ChallengesRating::Positive),
            Some(ChallengesRating::Neutral),
            Some(ChallengesRating::Negative),
            Some(ChallengesRating::Negative),
            None,
        ];

        assert_eq!(count_ratings(mixed), (1, 2));
    }

    /// Ten negative ratings hide the subtask, nine do not.
    #[test]
    fn ten_negative_ratings_hide_a_subtask() {
        for count in 0..AUTO_HIDE_NEGATIVE_RATINGS {
            let (positive, negative) = count_ratings(ratings(0, count));
            assert!(!should_auto_hide(positive, negative), "{count}");
        }

        let (positive, negative) = count_ratings(ratings(0, AUTO_HIDE_NEGATIVE_RATINGS));
        assert!(should_auto_hide(positive, negative));
    }

    /// A subtask most users like stays visible however often it is disliked.
    #[test]
    fn more_likes_than_dislikes_keep_a_subtask_visible() {
        let (positive, negative) = count_ratings(ratings(10, 10));
        assert!(!should_auto_hide(positive, negative));

        let (positive, negative) = count_ratings(ratings(10, 11));
        assert!(should_auto_hide(positive, negative));
    }
}
