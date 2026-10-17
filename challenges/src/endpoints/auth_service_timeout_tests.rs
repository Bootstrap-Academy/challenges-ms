//! A stalled auth service ends signed-in requests and the moderation basis
//! after `AUTH_TIMEOUT`. Isolated PostgreSQL/Redis; the auth service is a local
//! stub that accepts the request and never answers.
use std::time::Instant;

use lib::services::{auth::AUTH_TIMEOUT, ServiceError};
use serde_json::json;
use uuid::Uuid;

use super::heart_tests::Fixture;

/// A signed-in read of an attempt nobody made: `404` once the sign-in check
/// has passed.
async fn read_attempt(f: &Fixture) -> u16 {
    let app = f.app();
    let path = format!(
        "/tasks/{}/multiple_choice/{}/attempts/{}",
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4()
    );
    f.call(
        &app,
        Uuid::new_v4(),
        false,
        poem::http::Method::GET,
        &path,
        json!(null),
    )
    .await
    .0
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn sign_in_check_gives_up_on_a_stalled_auth_service_postgres() {
    let f = Fixture::new().await;
    assert_eq!(read_attempt(&f).await, 404);

    // Every signed-in route asks the auth service for the current authority
    // first. If it never answers, the request ends with the existing
    // "unavailable" answer instead of holding its transaction indefinitely.
    f.shop.lock().unwrap().stalled_authority = true;
    let started = Instant::now();
    let status = tokio::time::timeout(AUTH_TIMEOUT * 3, read_attempt(&f))
        .await
        .expect("the request is still waiting");
    assert_eq!(status, 503);
    assert!(started.elapsed() >= AUTH_TIMEOUT);
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn moderation_basis_gives_up_on_a_stalled_auth_service_postgres() {
    let f = Fixture::new().await;
    let creator = Uuid::new_v4();
    assert_eq!(
        f.state
            .services
            .auth
            .moderation_basis(creator)
            .await
            .unwrap(),
        json!({"recorded_acceptance":"synthetic","automatic_quality_basis_confirmed":false})
    );

    // Reports and feedback replace an error here with their fallback basis;
    // a stalled auth service now produces that error after the time limit.
    f.shop.lock().unwrap().stalled_basis = true;
    let started = Instant::now();
    let result = tokio::time::timeout(
        AUTH_TIMEOUT * 3,
        f.state.services.auth.moderation_basis(creator),
    )
    .await
    .expect("the moderation basis is still waiting");
    assert!(
        matches!(&result, Err(ServiceError::ReqwestError(err)) if err.is_timeout()),
        "{result:?}"
    );
    assert!(started.elapsed() >= AUTH_TIMEOUT);
}
