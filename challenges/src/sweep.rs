use std::{fmt::Display, time::Duration};

use lib::{
    config::Config,
    services::{auth::User, Services},
};
use sea_orm::{DatabaseConnection, TransactionTrait};
use tokio::time::{sleep_until, Instant};
use tracing::{info, warn};
use uuid::Uuid;

use crate::services::users::{delete_user_data, referenced_user_ids};

/// Delete the data of all users that do not exist in the auth microservice
/// anymore.
///
/// This is a safety net for accounts whose deletion has not been propagated to
/// this microservice, e.g. because it was unreachable at that time.
pub async fn sweep_deleted_users(
    db: &DatabaseConnection,
    services: &Services,
    config: &Config,
) -> anyhow::Result<()> {
    let interval = Duration::from_secs(1)
        .checked_div(config.deleted_user_sweep.rate_limit)
        .unwrap_or_default();

    let mut stats = Stats::default();
    let mut next_request = Instant::now();
    let mut after = Uuid::nil();
    loop {
        let user_ids = referenced_user_ids(db, after, config.deleted_user_sweep.batch_size).await?;
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
                Decision::Delete => {
                    stats.missing += 1;
                    let txn = db.begin().await?;
                    delete_user_data(&txn, user_id).await?;
                    txn.commit().await?;
                    stats.deleted += 1;
                }
                Decision::Skip => stats.errors += 1,
            }
        }
    }

    info!(
        "Checked {} users ({} missing, {} deleted, {} errors)",
        stats.checked, stats.missing, stats.deleted, stats.errors
    );
    Ok(())
}

/// What to do with the data of a user that is referenced in the database.
#[derive(Debug, PartialEq, Eq)]
enum Decision {
    /// The user still exists, so their data is kept.
    Keep,
    /// The user does not exist anymore, so their data is deleted.
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
}
