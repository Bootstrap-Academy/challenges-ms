//! The complete API as `main` assembles it, isolated PostgreSQL/Redis and a
//! local skills stub. No live service is contacted.
use std::time::Duration;

use crate::services::sandbox::SandboxClient as SandkastenClient;
use lib::jwt::{sign_jwt, InternalAuthToken, UserAccessToken, UserAccessTokenData};
use poem::{Endpoint, EndpointExt, IntoResponse, Request, Route};
use poem_ext::db::DbTransactionMiddleware;
use poem_openapi::OpenApiService;
use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use serde_json::{json, Value};
use uuid::Uuid;

use super::heart_tests::Fixture;

async fn startup_app(f: &Fixture) -> impl Endpoint {
    let api_service = OpenApiService::new(
        super::setup_api(
            f.state.clone(),
            f.config.clone(),
            SandkastenClient::new(f.config.challenges.coding_challenges.sandkasten_url.clone()),
        )
        .await
        .unwrap(),
        "Local lesson milestone regression",
        "1",
    );
    Route::new()
        .nest("/openapi.json", api_service.spec_endpoint())
        .nest("/", api_service)
        .with(DbTransactionMiddleware::new(f.state.db.clone()))
        .with(crate::services::hearts::SettlementMiddleware(
            f.state.clone(),
        ))
        .data(f.state.clone())
}

fn internal_token(f: &Fixture, audience: &'static str) -> String {
    sign_jwt(
        InternalAuthToken {
            aud: audience.into(),
        },
        f.state.internal_jwt_secrets.get(audience),
        Duration::from_secs(60),
    )
    .unwrap()
}

fn user_token(f: &Fixture, user: Uuid) -> String {
    sign_jwt(
        UserAccessToken {
            uid: user,
            rt: Uuid::new_v4().to_string(),
            data: UserAccessTokenData {
                admin: true,
                email_verified: true,
            },
        },
        &f.state.jwt_secret,
        Duration::from_secs(60),
    )
    .unwrap()
}

async fn call(
    app: &impl Endpoint,
    method: poem::http::Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (u16, Value) {
    let mut request = Request::builder().method(method).uri(path.parse().unwrap());
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    let request = match body {
        Some(body) => request
            .header("Content-Type", "application/json")
            .body(body.to_string()),
        None => request.finish(),
    };
    let mut response = match app.call(request).await {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    };
    let status = response.status().as_u16();
    let bytes = response.take_body().into_bytes().await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"raw":String::from_utf8_lossy(&bytes)})),
    )
}

async fn put(
    app: &impl Endpoint,
    token: &str,
    user: Uuid,
    unit: &str,
    body: Value,
) -> (u16, Value) {
    call(
        app,
        poem::http::Method::PUT,
        &format!("/_internal/lesson-milestones/{user}/{unit}"),
        Some(token),
        Some(body),
    )
    .await
}

async fn count(f: &Fixture, table: &str, user: Uuid) -> i64 {
    f.state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            format!("SELECT count(*) AS n FROM {table} WHERE user_id=$1"),
            [user.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap()
}

async fn rows(f: &Fixture, sql: &str, user: Uuid) -> Value {
    f.state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            format!("SELECT coalesce(jsonb_agg(to_jsonb(r)),'[]') AS value FROM ({sql}) r"),
            [user.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "value")
        .unwrap()
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn lesson_milestones_postgres() {
    let f = Fixture::new().await;
    let app = startup_app(&f).await;
    let internal = internal_token(&f, "challenges");
    let user = Uuid::new_v4();
    let body = json!({"skill_id":"synthetic-sub-skill","xp":20,"completion":"llm_verdict"});

    // The route is part of the served API description.
    let (status, spec) = call(&app, poem::http::Method::GET, "/openapi.json", None, None).await;
    assert_eq!(status, 200);
    assert!(spec["paths"]["/_internal/lesson-milestones/{user_id}/{unit_id}"]["put"].is_object());

    // Only an internal token issued for this service is accepted, never a user.
    for token in [
        None,
        Some(user_token(&f, user)),
        Some(internal_token(&f, "skills")),
    ] {
        let (status, _) = call(
            &app,
            poem::http::Method::PUT,
            &format!("/_internal/lesson-milestones/{user}/llm-lesson-1"),
            token.as_deref(),
            Some(body.clone()),
        )
        .await;
        assert_eq!(status, 401);
    }

    // Invalid reports record nothing.
    let mut statuses = Vec::new();
    for (unit, invalid) in [
        (
            "llm-lesson-1",
            json!({"skill_id":"synthetic-sub-skill","xp":0,"completion":"deterministic"}),
        ),
        (
            "llm-lesson-1",
            json!({"skill_id":"synthetic-sub-skill","xp":51,"completion":"deterministic"}),
        ),
        (
            "llm-lesson-1",
            json!({"skill_id":"synthetic-sub-skill","xp":5,"completion":"guessed"}),
        ),
        (
            "llm-lesson-1",
            json!({"skill_id":"synthetic-root","xp":5,"completion":"deterministic"}),
        ),
        ("Not_A_Unit", body.clone()),
    ] {
        statuses.push(put(&app, &internal, user, unit, invalid).await.0);
    }
    assert_eq!(statuses, [422, 422, 422, 404, 422]);
    assert_eq!(count(&f, "challenge_lesson_milestones", user).await, 0);
    assert_eq!(count(&f, "challenge_benefit_earnings", user).await, 0);

    // First report: one milestone, one earning and one XP component, no hearts, no coins.
    let (status, created) = put(&app, &internal, user, "llm-lesson-1", body.clone()).await;
    assert_eq!(status, 200, "{created}");
    assert_eq!(created["created"], true);
    assert_eq!(created["milestone"]["unit_id"], "llm-lesson-1");
    assert_eq!(created["milestone"]["skill_id"], "synthetic-sub-skill");
    assert_eq!(created["milestone"]["xp"], 20);
    assert_eq!(created["milestone"]["completion"], "llm_verdict");
    let earnings = rows(
        &f,
        "SELECT e.original,e.subtask_id=m.id AS keyed FROM challenge_benefit_earnings e JOIN challenge_lesson_milestones m ON m.user_id=e.user_id WHERE e.user_id=$1",
        user,
    )
    .await;
    assert_eq!(earnings.as_array().unwrap().len(), 1);
    assert_eq!(earnings[0]["keyed"], true);
    assert_eq!(earnings[0]["original"]["coins"], 0);
    assert_eq!(earnings[0]["original"]["xp"], 20);
    let components = rows(
        &f,
        "SELECT kind,request FROM challenge_benefit_components WHERE user_id=$1",
        user,
    )
    .await;
    assert_eq!(components.as_array().unwrap().len(), 1);
    assert_eq!(components[0]["kind"], "xp");
    assert_eq!(components[0]["request"]["skill_id"], "synthetic-sub-skill");
    assert_eq!(components[0]["request"]["xp"], 20);
    assert_eq!(count(&f, "challenge_heart_operations", user).await, 0);

    // Concurrent and repeated reports, even with other values, keep the original.
    let changed = json!({"skill_id":"other-sub-skill","xp":5,"completion":"deterministic"});
    let (a, b) = tokio::join!(
        put(&app, &internal, user, "llm-lesson-1", body.clone()),
        put(&app, &internal, user, "llm-lesson-1", changed.clone()),
    );
    for (status, repeated) in [a, b] {
        assert_eq!(status, 200);
        assert_eq!(repeated["created"], false);
        assert_eq!(repeated["milestone"], created["milestone"]);
    }
    assert_eq!(count(&f, "challenge_lesson_milestones", user).await, 1);
    assert_eq!(count(&f, "challenge_benefit_earnings", user).await, 1);
    assert_eq!(count(&f, "challenge_benefit_components", user).await, 1);

    // Another unit, as in a replacement course, earns again; other users are independent.
    let (status, second) = put(&app, &internal, user, "llm-lesson-2", changed.clone()).await;
    assert_eq!((status, second["created"].clone()), (200, json!(true)));
    let other = Uuid::new_v4();
    let (status, theirs) = put(&app, &internal, other, "llm-lesson-1", body.clone()).await;
    assert_eq!((status, theirs["created"].clone()), (200, json!(true)));
    assert_eq!(count(&f, "challenge_benefit_earnings", user).await, 2);
    assert_eq!(count(&f, "challenge_benefit_earnings", other).await, 1);

    // The existing dispatcher delivers exactly these awards to skills-ms.
    crate::services::benefits::dispatch(&f.state.db, &f.state.services)
        .await
        .unwrap();
    let states = rows(
        &f,
        "SELECT id,state,request FROM challenge_benefit_components WHERE user_id=$1 ORDER BY request->>'skill_id'",
        user,
    )
    .await;
    assert_eq!(states.as_array().unwrap().len(), 2);
    {
        let shop = f.shop.lock().unwrap();
        for (component, (skill, xp)) in states
            .as_array()
            .unwrap()
            .iter()
            .zip([("other-sub-skill", 5), ("synthetic-sub-skill", 20)])
        {
            assert_eq!(component["state"], "applied");
            let id: Uuid = component["id"].as_str().unwrap().parse().unwrap();
            let delivered = &shop.xp_operations[&id];
            assert_eq!(delivered["user_id"], json!(user));
            assert_eq!(delivered["skill_id"], skill);
            assert_eq!(delivered["xp"], xp);
            assert_eq!(delivered["earning_id"], component["request"]["earning_id"]);
        }
    }

    // Export, then erasure: milestones go with the account; a late report is refused.
    let tx = f.state.db.begin().await.unwrap();
    let export = crate::services::users::export_user_data(&tx, user)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let units: Vec<_> = export
        .lesson_milestones
        .iter()
        .map(|m| m.unit_id.as_str())
        .collect();
    assert_eq!(units, ["llm-lesson-1", "llm-lesson-2"]);
    let (status, _) = call(
        &app,
        poem::http::Method::DELETE,
        &format!("/_internal/users/{user}"),
        Some(&internal),
        None,
    )
    .await;
    assert_eq!(status, 204);
    assert_eq!(count(&f, "challenge_lesson_milestones", user).await, 0);
    let (status, _) = put(&app, &internal, user, "llm-lesson-3", body.clone()).await;
    assert_eq!(status, 410);
    assert_eq!(count(&f, "challenge_lesson_milestones", user).await, 0);
    assert_eq!(count(&f, "challenge_lesson_milestones", other).await, 1);
}
