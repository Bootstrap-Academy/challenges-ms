use std::{fmt::Display, time::Duration};

use anyhow::bail;
use lib::{
    config::DeletedUserSweep,
    services::{auth::User, Services},
    Cache,
};
use sea_orm::{DatabaseConnection, TransactionTrait};
use tokio::time::{sleep_until, Instant};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::services::users::{delete_user_data, referenced_user_ids};

/// Delete the data of all users that do not exist in the auth microservice
/// anymore.
///
/// This is a safety net for accounts whose deletion has not been propagated to
/// this microservice, e.g. because it was unreachable at that time.
///
/// Every referenced user is looked up before anything is deleted. If more of
/// them are missing, or more of the lookups fail, than [`abort_limit`] allows,
/// the run deletes nothing and fails. A broken auth microservice is then more
/// likely than that many lost deletions, and a run that cannot look up most
/// users would otherwise end as if everything were fine.
pub async fn sweep_deleted_users(
    db: &DatabaseConnection,
    services: &Services,
    cache: &Cache,
    config: &DeletedUserSweep,
) -> anyhow::Result<()> {
    let interval = Duration::from_secs(1)
        .checked_div(config.rate_limit)
        .unwrap_or_default();

    let mut stats = Stats::default();
    let mut missing = Vec::new();
    let mut next_request = Instant::now();
    let mut after = Uuid::nil();
    loop {
        let user_ids = referenced_user_ids(db, after, config.batch_size).await?;
        let Some(&last) = user_ids.last() else {
            break;
        };
        after = last;

        for user_id in user_ids {
            sleep_until(next_request).await;
            next_request = Instant::now() + interval;

            stats.checked += 1;
            match decide(services.auth.get_user_by_id_uncached(user_id).await) {
                Decision::Keep => {}
                Decision::Delete => missing.push(user_id),
                Decision::Skip => stats.errors += 1,
            }
        }
    }
    stats.missing = missing.len() as u64;

    let limit = abort_limit(config, stats.checked);
    if stats.exceeds(limit) {
        stats.log();
        error!(
            "Deleted user sweep aborted, nothing has been deleted: of {} users, {} are missing \
             in the auth microservice and {} could not be looked up; each may be at most \
             {limit}. Check the auth microservice. If the accounts really have been deleted \
             and the failed lookups are understood, raise deleted_user_sweep.max_missing for \
             one run.",
            stats.checked, stats.missing, stats.errors
        );
        bail!(
            "of {} users, {} are missing and {} could not be looked up (at most {limit} each); \
             nothing has been deleted",
            stats.checked,
            stats.missing,
            stats.errors
        );
    }

    for user_id in missing {
        let result: anyhow::Result<()> = async {
            let txn = db.begin().await?;
            delete_user_data(&txn, user_id).await?;
            // Match the endpoint: failed cache invalidation leaves referenced
            // rows for a subsequent safety-net pass to find again.
            cache.pop_tag(&user_id.to_string()).await?;
            txn.commit().await?;
            Ok(())
        }
        .await;
        match result {
            Ok(()) => stats.deleted += 1,
            Err(_) => {
                stats.errors += 1;
                warn!("User erasure failed; continuing sweep");
            }
        }
    }

    stats.log();
    Ok(())
}

/// The largest number of missing users, and separately of failed lookups, with
/// which a run still deletes.
///
/// A normal run finds a handful of missing users among thousands and no failed
/// lookups, while an auth microservice that has lost its users (an empty or
/// wrong database, for example) reports most of them as missing, and one that
/// is down or has moved fails most lookups. The share keeps the limit in
/// proportion to the number of users, and the absolute number keeps a small
/// database from tripping over a few deletions.
fn abort_limit(config: &DeletedUserSweep, checked: u64) -> u64 {
    // `as` saturates, so a negative share counts as 0.
    let share = (checked as f64 * config.max_missing_share).floor() as u64;
    config.max_missing.max(share)
}

/// What to do with the data of a user that is referenced in the database.
#[derive(Debug, PartialEq, Eq)]
enum Decision {
    /// The user still exists, so their data is kept.
    Keep,
    /// The user does not exist anymore, so their data is deleted, unless too
    /// many users are missing.
    Delete,
    /// The user could not be looked up, so their data is kept for now.
    Skip,
}

fn decide<E: Display>(user: Result<Option<User>, E>) -> Decision {
    match user {
        Ok(Some(_)) => Decision::Keep,
        Ok(None) => Decision::Delete,
        Err(err) => {
            warn!("Could not look up user in auth microservice: {err}");
            Decision::Skip
        }
    }
}

#[derive(Debug, Default)]
struct Stats {
    checked: u64,
    missing: u64,
    deleted: u64,
    errors: u64,
}

impl Stats {
    /// Whether the lookups went so wrong that the run must not delete anything:
    /// more users are missing, or more lookups have failed, than `limit`.
    ///
    /// Asked before the deletions, so `errors` holds failed lookups only.
    fn exceeds(&self, limit: u64) -> bool {
        self.missing > limit || self.errors > limit
    }

    fn log(&self) {
        info!(
            "Checked {} users ({} missing, {} deleted, {} errors)",
            self.checked, self.missing, self.deleted, self.errors
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> User {
        User {
            id: Uuid::new_v4(),
            name: "user".into(),
            display_name: "User".into(),
            avatar_url: None,
            registration: 0.0,
            admin: false,
            leaderboard_opt_out: false,
        }
    }

    fn limits(max_missing: u64, max_missing_share: f64) -> DeletedUserSweep {
        DeletedUserSweep {
            max_missing,
            max_missing_share,
            ..Default::default()
        }
    }

    #[test]
    fn test_decide_existing_user() {
        assert_eq!(decide::<&str>(Ok(Some(user()))), Decision::Keep);
    }

    #[test]
    fn test_decide_deleted_user() {
        assert_eq!(decide::<&str>(Ok(None)), Decision::Delete);
    }

    #[test]
    fn test_decide_auth_service_error() {
        assert_eq!(
            decide::<&str>(Err("unexpected response status code: 500")),
            Decision::Skip
        );
    }

    fn after_lookups(missing: u64, errors: u64) -> Stats {
        Stats {
            checked: 10_000,
            missing,
            deleted: 0,
            errors,
        }
    }

    #[test]
    fn test_abort_limit_grows_with_the_number_of_users() {
        let config = DeletedUserSweep::default();
        assert_eq!(abort_limit(&config, 10_000), 100);
        assert_eq!(abort_limit(&config, 10_099), 100);
        assert_eq!(abort_limit(&config, 1_000_000), 10_000);
    }

    #[test]
    fn test_abort_limit_never_falls_below_the_absolute_number() {
        let config = DeletedUserSweep::default();
        assert_eq!(abort_limit(&config, 0), 25);
        assert_eq!(abort_limit(&config, 100), 25);
        assert_eq!(abort_limit(&config, 2_599), 25);
        assert_eq!(abort_limit(&config, 2_600), 26);
    }

    #[test]
    fn test_abort_limit_follows_the_configuration() {
        assert_eq!(abort_limit(&limits(0, 0.0), 10_000), 0);
        assert_eq!(abort_limit(&limits(3, 0.0), 10_000), 3);
        assert_eq!(abort_limit(&limits(0, 0.5), 10_000), 5_000);
        assert_eq!(abort_limit(&limits(0, -1.0), 10_000), 0);
        assert_eq!(abort_limit(&limits(7, f64::NAN), 10_000), 7);
    }

    #[test]
    fn test_too_many_missing_users_stop_the_run() {
        assert!(after_lookups(101, 0).exceeds(100));
        assert!(!after_lookups(100, 0).exceeds(100));
    }

    #[test]
    fn test_too_many_failed_lookups_stop_the_run() {
        assert!(after_lookups(0, 101).exceeds(100));
        assert!(!after_lookups(0, 100).exceeds(100));
        // An auth microservice that is down fails every lookup.
        assert!(after_lookups(0, 10_000).exceeds(100));
    }

    #[test]
    fn test_a_normal_run_goes_on() {
        assert!(!after_lookups(0, 0).exceeds(100));
        assert!(!after_lookups(11, 3).exceeds(100));
        assert!(!after_lookups(100, 100).exceeds(100));
    }
}
