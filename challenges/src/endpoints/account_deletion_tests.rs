//! Account deletion removes the first-completion records of a user with their
//! delivery components and observations and leaves no trace of the user in
//! shared tasks; the nightly sweep deletes only users the auth service really
//! reports as gone. Isolated PostgreSQL/Redis; the dispatcher runs with a local
//! destination, the auth service is a local stub and no live service is
//! contacted.
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use lib::{
    config::DeletedUserSweep,
    jwt::{sign_jwt, InternalAuthToken},
};
use poem::{http::Method, Endpoint, EndpointExt, IntoResponse, Request, Route};
use poem_ext::db::DbTransactionMiddleware;
use poem_openapi::OpenApiService;
use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use serde_json::{json, Value};
use tokio::sync::Notify;
use uuid::Uuid;

use super::heart_tests::Fixture;
use crate::services::{benefits, users};

const SKILLS: &[&str] = &["synthetic-sub-skill", "other-sub-skill"];

fn internal_app(f: &Fixture) -> impl Endpoint {
    Route::new()
        .nest(
            "/",
            OpenApiService::new(
                super::internal::Internal {
                    state: f.state.clone(),
                    config: f.config.clone(),
                },
                "Local account deletion regression",
                "1",
            ),
        )
        .with(DbTransactionMiddleware::new(f.state.db.clone()))
        .data(f.state.clone())
}

/// `DELETE /_internal/users/{user}` as the auth service calls it.
async fn delete_account(f: &Fixture, app: &impl Endpoint, user: Uuid) -> u16 {
    let token = sign_jwt(
        InternalAuthToken {
            aud: "challenges".into(),
        },
        f.state.internal_jwt_secrets.get("challenges"),
        Duration::from_secs(60),
    )
    .unwrap();
    let request = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/_internal/users/{user}").parse().unwrap())
        .header("Authorization", format!("Bearer {token}"))
        .finish();
    match app.call(request).await {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
    .status()
    .as_u16()
}

/// A first completion through the producer, as a solved task or a completed
/// lesson unit records it: one earning and one XP component per skill.
async fn earn(f: &Fixture, user: Uuid, xp: i64, skills: &[&str]) {
    let tx = f.state.db.begin().await.unwrap();
    benefits::lock_subject(&tx, user).await.unwrap();
    benefits::record(
        &tx,
        user,
        Uuid::new_v4(),
        xp,
        0,
        skills.iter().map(|skill| skill.to_string()).collect(),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

/// Run the real dispatcher until nothing is due, with a destination that
/// applies every component. Returns the recipients it delivered to.
async fn deliver_all(f: &Fixture) -> Vec<Uuid> {
    let recipients = Arc::new(Mutex::new(Vec::new()));
    while benefits::dispatch_with(&f.state.db, |id, user, _kind, _request| {
        let recipients = recipients.clone();
        async move {
            recipients.lock().unwrap().push(user);
            Ok(json!({"state":"applied","operation_id":id}))
        }
    })
    .await
    .unwrap()
        > 0
    {}
    let recipients = recipients.lock().unwrap().clone();
    recipients
}

async fn value(f: &Fixture, sql: &str, parameter: sea_orm::Value) -> Value {
    f.state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            format!("SELECT to_jsonb(r) AS value FROM ({sql}) r"),
            [parameter],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "value")
        .unwrap()
}

/// Rows of a user in the earnings, components and observations tables.
async fn benefit_rows(f: &Fixture, user: Uuid) -> Value {
    value(
        f,
        "SELECT (SELECT count(*) FROM challenge_benefit_earnings WHERE user_id=$1) AS earnings,
                (SELECT count(*) FROM challenge_benefit_components WHERE user_id=$1) AS components,
                (SELECT count(*) FROM challenge_benefit_observations o JOIN challenge_benefit_components c ON c.id=o.component_id WHERE c.user_id=$1) AS observations",
        user.into(),
    )
    .await
}

fn rows(earnings: u64, components: u64, observations: u64) -> Value {
    json!({"earnings":earnings,"components":components,"observations":observations})
}

/// The IDs of a user's components, to find their observations afterwards.
async fn component_ids(f: &Fixture, user: Uuid) -> Value {
    value(
        f,
        "SELECT coalesce(jsonb_agg(id),'[]') AS ids FROM challenge_benefit_components WHERE user_id=$1",
        user.into(),
    )
    .await["ids"]
        .clone()
}

/// Observations that still refer to any of the given component IDs.
async fn observations_of(f: &Fixture, components: &Value) -> Value {
    value(
        f,
        "SELECT count(*) AS n FROM challenge_benefit_observations WHERE component_id IN (SELECT jsonb_array_elements_text($1::jsonb)::uuid)",
        components.clone().into(),
    )
    .await["n"]
        .clone()
}

async fn execute(f: &Fixture, sql: &str, values: Vec<sea_orm::Value>) {
    f.state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            sql,
            values,
        ))
        .await
        .unwrap();
}

/// A task of a lecture as the first user who added a subtask there created it.
async fn course_task(f: &Fixture, creator: Uuid) -> Uuid {
    let task = Uuid::new_v4();
    execute(
        f,
        "INSERT INTO challenges_tasks(id,creator,creation_timestamp) VALUES($1,$2,now())",
        vec![task.into(), creator.into()],
    )
    .await;
    execute(
        f,
        "INSERT INTO challenges_course_tasks(task_id,course_id,section_id,lecture_id) VALUES($1,'synthetic-course',$2,'lecture')",
        vec![task.into(), task.to_string().into()],
    )
    .await;
    task
}

/// A question of `creator` in `task`.
async fn subtask(f: &Fixture, task: Uuid, creator: Uuid) {
    let subtask = Uuid::new_v4();
    execute(
        f,
        "INSERT INTO challenges_subtasks(id,task_id,creator,creation_timestamp,xp,coins,enabled,retired,ty) VALUES($1,$2,$3,now(),0,0,true,false,'question')",
        vec![subtask.into(), task.into(), creator.into()],
    )
    .await;
    execute(
        f,
        "INSERT INTO challenges_questions(subtask_id,question,answers,case_sensitive,ascii_letters,digits,punctuation,blocks) VALUES($1,'Answer',ARRAY['yes'],false,true,true,true,ARRAY[]::text[])",
        vec![subtask.into()],
    )
    .await;
}

/// The creator of a task, or `None` if the task is gone.
async fn task_creator(f: &Fixture, task: Uuid) -> Option<Uuid> {
    f.state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT creator FROM challenges_tasks WHERE id=$1",
            [task.into()],
        ))
        .await
        .unwrap()
        .map(|row| row.try_get("", "creator").unwrap())
}

async fn subtasks_in(f: &Fixture, task: Uuid) -> Value {
    value(
        f,
        "SELECT count(*) AS n FROM challenges_subtasks WHERE task_id=$1",
        task.into(),
    )
    .await["n"]
        .clone()
}

/// Whether the sweep, which walks the referenced user IDs in ascending order,
/// finds the user.
async fn is_referenced(f: &Fixture, user: Uuid) -> bool {
    let before = Uuid::from_u128(user.as_u128() - 1);
    users::referenced_user_ids(&f.state.db, before, 1)
        .await
        .unwrap()
        .first()
        == Some(&user)
}

/// The nightly sweep without pauses between the lookups, which deletes at most
/// `max_missing` users.
async fn sweep(f: &Fixture, max_missing: u64) -> anyhow::Result<()> {
    let config = DeletedUserSweep {
        rate_limit: 0,
        max_missing,
        max_missing_share: 0.0,
        ..Default::default()
    };
    crate::sweep::sweep_deleted_users(&f.state.db, &f.state.services, &f.state.cache, &config).await
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn account_deletion_removes_first_completion_records_postgres() {
    let f = Fixture::new().await;
    let app = internal_app(&f);
    let user = Uuid::new_v4();
    let other = Uuid::new_v4();

    // A delivered first completion, one without XP and therefore without
    // components, and one whose components still wait for delivery.
    earn(&f, user, 20, SKILLS).await;
    earn(&f, user, 0, SKILLS).await;
    earn(&f, other, 20, SKILLS).await;
    deliver_all(&f).await;
    earn(&f, user, 20, SKILLS).await;
    assert_eq!(benefit_rows(&f, user).await, rows(3, 4, 2));
    assert_eq!(benefit_rows(&f, other).await, rows(1, 2, 2));
    let components = component_ids(&f, user).await;
    assert_eq!(components.as_array().unwrap().len(), 4);

    // The deletion works in the caller's transaction: nothing is gone before
    // the commit, and a rollback keeps everything.
    let tx = f.state.db.begin().await.unwrap();
    users::delete_user_data(&tx, user).await.unwrap();
    assert_eq!(benefit_rows(&f, user).await, rows(3, 4, 2));
    tx.rollback().await.unwrap();
    assert_eq!(benefit_rows(&f, user).await, rows(3, 4, 2));

    // The route removes the records and every part of them, for this user only.
    assert_eq!(delete_account(&f, &app, user).await, 204);
    assert_eq!(benefit_rows(&f, user).await, rows(0, 0, 0));
    assert_eq!(observations_of(&f, &components).await, json!(0));
    assert_eq!(benefit_rows(&f, other).await, rows(1, 2, 2));

    // The export has nothing left, the undelivered components are never
    // delivered, and a repeated call from the auth service still succeeds.
    let tx = f.state.db.begin().await.unwrap();
    let export = users::export_user_data(&tx, user).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        export.benefits,
        json!({"earnings":[],"components":[],"observations":[]})
    );
    assert!(!deliver_all(&f).await.contains(&user));
    assert_eq!(delete_account(&f, &app, user).await, 204);
    assert_eq!(benefit_rows(&f, other).await, rows(1, 2, 2));
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn account_deletion_waits_for_a_running_delivery_postgres() {
    let f = Fixture::new().await;
    // Nothing else is due, so the dispatcher picks exactly this component.
    deliver_all(&f).await;
    let user = Uuid::new_v4();
    earn(&f, user, 10, &SKILLS[..1]).await;
    let components = component_ids(&f, user).await;

    // The delivery holds its component while it waits for the remote reply and
    // records an observation before it commits. The deletion starts meanwhile.
    let holding = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let dispatcher = benefits::dispatch_with(&f.state.db, |id, recipient, _kind, _request| {
        let holding = holding.clone();
        let release = release.clone();
        async move {
            if recipient == user {
                holding.notify_one();
                release.notified().await;
            }
            Ok(json!({"state":"applied","operation_id":id}))
        }
    });
    let deletion = async {
        holding.notified().await;
        let deletion = async {
            let tx = f.state.db.begin().await?;
            users::delete_user_data(&tx, user).await?;
            tx.commit().await
        };
        tokio::pin!(deletion);
        assert!(
            tokio::time::timeout(Duration::from_millis(300), &mut deletion)
                .await
                .is_err(),
            "the deletion waits for the running delivery"
        );
        release.notify_one();
        deletion.await
    };
    let (delivered, deleted) = tokio::join!(dispatcher, deletion);
    assert_eq!(delivered.unwrap(), 1);
    deleted.unwrap();

    // The observation the delivery wrote while the deletion waited is gone too.
    assert_eq!(benefit_rows(&f, user).await, rows(0, 0, 0));
    assert_eq!(observations_of(&f, &components).await, json!(0));
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn sweep_reaches_records_left_by_an_earlier_deletion_postgres() {
    let f = Fixture::new().await;
    // What an account deleted before this fix left behind: its erasure event
    // and its delivered first completions, and nothing else.
    let user = Uuid::new_v4();
    earn(&f, user, 20, SKILLS).await;
    deliver_all(&f).await;
    f.state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO moderation_erasure_events(subject) VALUES($1)",
            [user.into()],
        ))
        .await
        .unwrap();
    assert_eq!(benefit_rows(&f, user).await, rows(1, 2, 2));

    // The nightly sweep walks the referenced user IDs in ascending order and
    // reaches the account through these records alone.
    let before = Uuid::from_u128(user.as_u128() - 1);
    assert_eq!(
        users::referenced_user_ids(&f.state.db, before, 1)
            .await
            .unwrap(),
        [user]
    );

    // For an account the auth service no longer knows, the sweep runs the
    // same deletion, which also accepts the existing erasure event.
    let tx = f.state.db.begin().await.unwrap();
    users::delete_user_data(&tx, user).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(benefit_rows(&f, user).await, rows(0, 0, 0));
    assert_ne!(
        users::referenced_user_ids(&f.state.db, before, 1)
            .await
            .unwrap()
            .first(),
        Some(&user)
    );
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn account_deletion_resets_the_creator_of_shared_tasks_postgres() {
    let f = Fixture::new().await;
    let app = internal_app(&f);
    let user = Uuid::new_v4();
    let other = Uuid::new_v4();

    // A task the user started that holds a subtask of somebody else, one with
    // only the user's own subtask and one without any.
    let shared = course_task(&f, user).await;
    subtask(&f, shared, other).await;
    let own = course_task(&f, user).await;
    subtask(&f, own, user).await;
    let empty = course_task(&f, user).await;

    assert_eq!(delete_account(&f, &app, user).await, 204);

    // The shared task stays for the other subtask, with the nil uuid as its
    // creator; the two others go with the account as before.
    assert_eq!(task_creator(&f, shared).await, Some(Uuid::nil()));
    assert_eq!(subtasks_in(&f, shared).await, json!(1));
    assert_eq!(task_creator(&f, own).await, None);
    assert_eq!(task_creator(&f, empty).await, None);

    // Nothing refers to the account anymore, so the nightly sweep does not
    // find it again and run its deletion once more.
    assert!(!is_referenced(&f, user).await);
    assert!(is_referenced(&f, other).await);
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn sweep_finishes_an_earlier_deletion_and_then_leaves_the_account_alone_postgres() {
    let f = Fixture::new().await;
    // What an account deleted before shared tasks lost their creator left
    // behind: its erasure event, its first completions and a shared task.
    let gone = Uuid::new_v4();
    let author = Uuid::new_v4();
    earn(&f, gone, 20, SKILLS).await;
    deliver_all(&f).await;
    execute(
        &f,
        "INSERT INTO moderation_erasure_events(subject) VALUES($1)",
        vec![gone.into()],
    )
    .await;
    let shared = course_task(&f, gone).await;
    subtask(&f, shared, author).await;
    f.shop.lock().unwrap().deleted_users.insert(gone);

    sweep(&f, u64::MAX).await.unwrap();
    assert_eq!(benefit_rows(&f, gone).await, rows(0, 0, 0));
    assert_eq!(task_creator(&f, shared).await, Some(Uuid::nil()));
    assert_eq!(subtasks_in(&f, shared).await, json!(1));
    assert!(!is_referenced(&f, gone).await);
    assert!(is_referenced(&f, author).await);

    // The next run does not even ask about the account anymore.
    let lookups = std::mem::take(&mut f.shop.lock().unwrap().user_lookups);
    assert!(lookups.contains(&gone) && lookups.contains(&author));
    sweep(&f, u64::MAX).await.unwrap();
    let lookups = std::mem::take(&mut f.shop.lock().unwrap().user_lookups);
    assert!(!lookups.contains(&gone) && lookups.contains(&author));
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn sweep_deletes_nothing_when_the_user_route_is_missing_postgres() {
    let f = Fixture::new().await;
    let user = Uuid::new_v4();
    earn(&f, user, 20, SKILLS).await;
    deliver_all(&f).await;

    // A backend release that renamed the route answers 404 for every user,
    // without the error that says the user does not exist. The limit is off,
    // so only the answer itself decides.
    f.shop.lock().unwrap().user_route_missing = true;
    sweep(&f, u64::MAX).await.unwrap();

    assert!(f.shop.lock().unwrap().user_lookups.contains(&user));
    assert_eq!(benefit_rows(&f, user).await, rows(1, 2, 2));
    assert!(is_referenced(&f, user).await);
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn sweep_aborts_without_deleting_when_too_many_users_are_missing_postgres() {
    let f = Fixture::new().await;
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let author = Uuid::new_v4();
    earn(&f, first, 20, SKILLS).await;
    earn(&f, second, 20, SKILLS).await;
    deliver_all(&f).await;
    let shared = course_task(&f, first).await;
    subtask(&f, shared, author).await;
    f.shop.lock().unwrap().deleted_users.extend([first, second]);

    // The auth service reports both as deleted, with the right error, but the
    // limit is one. Both are looked up before anything is deleted, and nothing
    // is.
    let error = sweep(&f, 1).await.unwrap_err();
    assert!(
        error.to_string().contains("nothing has been deleted"),
        "{error}"
    );
    for user in [first, second] {
        assert!(f.shop.lock().unwrap().user_lookups.contains(&user));
        assert_eq!(benefit_rows(&f, user).await, rows(1, 2, 2));
        assert!(is_referenced(&f, user).await);
    }
    assert_eq!(task_creator(&f, shared).await, Some(first));

    // Once somebody has checked that these accounts are really gone, a run with
    // a higher limit deletes them.
    sweep(&f, 2).await.unwrap();
    for user in [first, second] {
        assert_eq!(benefit_rows(&f, user).await, rows(0, 0, 0));
        assert!(!is_referenced(&f, user).await);
    }
    assert_eq!(task_creator(&f, shared).await, Some(Uuid::nil()));
}
