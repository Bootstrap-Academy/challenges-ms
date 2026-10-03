//! Native read-only history contract: synthetic PostgreSQL data and real JWTs.
use std::{collections::HashMap, sync::Arc, time::Duration};

use lib::jwt::{sign_jwt, InternalAuthToken, InternalJwtSecrets, JwtSecret};
use poem::{http::Method, Endpoint, EndpointExt, IntoResponse, Request, Route};
use poem_ext::db::DbTransactionMiddleware;
use poem_openapi::OpenApiService;
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::{json, Value};
use uuid::Uuid;

use super::heart_tests::Fixture;

fn app(f: &Fixture) -> impl Endpoint {
    Route::new()
        .nest(
            "/",
            OpenApiService::new(
                super::internal::Internal {
                    state: f.state.clone(),
                    config: f.config.clone(),
                },
                "Historical participation regression",
                "1",
            ),
        )
        .with(DbTransactionMiddleware::new(f.state.db.clone()))
        .with(crate::services::hearts::SettlementMiddleware(
            f.state.clone(),
        ))
        .data(f.state.clone())
}

fn token(f: &Fixture) -> String {
    sign_jwt(
        InternalAuthToken {
            aud: "challenges".into(),
        },
        f.state.internal_jwt_secrets.get("challenges"),
        Duration::from_secs(60),
    )
    .unwrap()
}

async fn call(app: &impl Endpoint, user: Uuid, token: Option<&str>, body: Value) -> (u16, Value) {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(
            format!("/_internal/users/{user}/learning-history")
                .parse()
                .unwrap(),
        )
        .header("Content-Type", "application/json");
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    let mut response = match app.call(request.body(body.to_string())).await {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    };
    let status = response.status().as_u16();
    let bytes = response.take_body().into_bytes().await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"raw": String::from_utf8_lossy(&bytes)})),
    )
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

async fn attempt(f: &Fixture, user: Uuid, subtask: Uuid, kind: &str, solved: bool) {
    let (table, column) = match kind {
        "matching" => ("challenges_matching_attempts", "matching_id"),
        "question" => ("challenges_question_attempts", "question_id"),
        "multiple_choice_question" => ("challenges_multiple_choice_attempts", "question_id"),
        "coding_challenge" => {
            execute(f, "INSERT INTO challenges_coding_challenge_submissions(id,subtask_id,creator,creation_timestamp,environment,code,judge_pending) VALUES($1,$2,$3,now(),'python','private submitted code',true)", vec![Uuid::new_v4().into(), subtask.into(), user.into()]).await;
            return;
        }
        _ => panic!("unsupported fixture"),
    };
    execute(
        f,
        &format!(
            "INSERT INTO {table}(id,{column},user_id,timestamp,solved) VALUES($1,$2,$3,now(),$4)"
        ),
        vec![
            Uuid::new_v4().into(),
            subtask.into(),
            user.into(),
            solved.into(),
        ],
    )
    .await;
}

async fn snapshot(f: &Fixture) -> Value {
    let mut tables = serde_json::Map::new();
    for table in [
        "challenges_user_subtasks",
        "challenges_multiple_choice_attempts",
        "challenges_matching_attempts",
        "challenges_question_attempts",
        "challenges_coding_challenge_submissions",
        "challenges_coding_challenge_result",
        "challenge_heart_operations",
        "challenge_benefit_earnings",
        "challenge_benefit_components",
    ] {
        let value = f.state.db.query_one(Statement::from_string(DbBackend::Postgres,
            format!("SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY to_jsonb(t)::text),'[]'::jsonb) AS value FROM {table} t")))
            .await.unwrap().unwrap().try_get::<Value>("", "value").unwrap();
        tables.insert(table.into(), value);
    }
    Value::Object(tables)
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn learning_history_participation_is_private_and_read_only_postgres() {
    let f = Fixture::new().await;
    let user = Uuid::new_v4();
    let foreign = Uuid::new_v4();
    let mut expected = Vec::new();
    let mut absent = Vec::new();
    for (kind, solved) in [
        ("multiple_choice_question", false),
        ("matching", true),
        ("question", false),
        ("coding_challenge", false),
    ] {
        let (_, mine) = f.seed(kind).await;
        let (_, theirs) = f.seed(kind).await;
        attempt(&f, user, mine, kind, solved).await;
        attempt(&f, foreign, theirs, kind, solved).await;
        expected.push(mine);
        absent.push(theirs);
    }
    // Aggregates from older histories also count if raw attempts are absent.
    for evidence in [
        "attempts=1",
        "last_attempt_timestamp=now()",
        "solved_timestamp=now()",
    ] {
        let (_, id) = f.seed("question").await;
        execute(
            &f,
            "INSERT INTO challenges_user_subtasks(user_id,subtask_id) VALUES($1,$2)",
            vec![user.into(), id.into()],
        )
        .await;
        execute(
            &f,
            &format!(
                "UPDATE challenges_user_subtasks SET {evidence} WHERE user_id=$1 AND subtask_id=$2"
            ),
            vec![user.into(), id.into()],
        )
        .await;
        expected.push(id);
    }
    // Empty or rating-only records are not a start; unattempted/unknown IDs stay absent.
    for rating in [false, true] {
        let (_, id) = f.seed("question").await;
        execute(&f, "INSERT INTO challenges_user_subtasks(user_id,subtask_id,rating,rating_timestamp) VALUES($1,$2,CASE WHEN $3 THEN 'positive'::challenges_rating END,CASE WHEN $3 THEN now() END)", vec![user.into(), id.into(), rating.into()]).await;
        absent.push(id);
    }
    absent.push(f.seed("question").await.1);
    absent.push(Uuid::new_v4());

    // Preserve actual results, a settled heart receipt and first-success XP too.
    let (task, id) = f.seed("multiple_choice_question").await;
    execute(&f, "INSERT INTO challenges_course_tasks(task_id,course_id,section_id,lecture_id) VALUES($1,'synthetic-course','section','lecture')", vec![task.into()]).await;
    execute(
        &f,
        "UPDATE challenges_subtasks SET xp=9 WHERE id=$1",
        vec![id.into()],
    )
    .await;
    f.shop.lock().unwrap().balances.insert(user, 4);
    let routes = f.app();
    for answers in [json!([false, true]), json!([true, false])] {
        let (status, body) = f
            .call(
                &routes,
                user,
                false,
                Method::POST,
                &format!("/tasks/{task}/multiple_choice/{id}/attempts"),
                json!({"answers": answers}),
            )
            .await;
        assert_eq!(status, 201, "{body}");
    }
    expected.push(id);
    expected.sort_unstable();
    let mut requested = expected.clone();
    requested.extend(absent);
    requested.push(id); // Deduplication must not leak duplicate history.
    let before = snapshot(&f).await;
    assert!(!before["challenge_heart_operations"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!before["challenge_benefit_components"]
        .as_array()
        .unwrap()
        .is_empty());
    let access_count = f.shop.lock().unwrap().access_requests.len();
    f.shop.lock().unwrap().access_status = Some(503);
    f.shop.lock().unwrap().policy_status = Some(503);
    let app = app(&f);
    let token = token(&f);
    for _ in 0..2 {
        assert_eq!(
            call(&app, user, Some(&token), json!({"subtask_ids": requested})).await,
            (
                200,
                json!({"attempted_subtask_ids":expected,"attempted_lecture_bindings":[]})
            )
        );
    }
    assert_eq!(
        call(&app, user, Some(&token), json!({"subtask_ids":[id]})).await,
        (
            200,
            json!({"attempted_subtask_ids":[id],"attempted_lecture_bindings":[]})
        )
    );
    assert_eq!(
        call(
            &app,
            Uuid::new_v4(),
            Some(&token),
            json!({"subtask_ids": requested})
        )
        .await,
        (
            200,
            json!({"attempted_subtask_ids":[],"attempted_lecture_bindings":[]})
        )
    );
    assert_eq!(snapshot(&f).await, before);
    assert_eq!(f.shop.lock().unwrap().access_requests.len(), access_count);
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn learning_history_uses_exact_requested_lecture_bindings_postgres() {
    let f = Fixture::new().await;
    let user = Uuid::new_v4();
    let foreign = Uuid::new_v4();
    for (course, lecture, participant) in [
        ("course-a", Some("lecture-1"), user),
        ("course-a", Some("lecture-2"), foreign),
        ("course-b", Some("lecture-1"), foreign),
        ("course-a", None, user),
    ] {
        let (task, id) = f.seed("question").await;
        execute(&f, "INSERT INTO challenges_course_tasks(task_id,course_id,section_id,lecture_id) VALUES($1,$2,'section',$3)", vec![task.into(), course.into(), lecture.into()]).await;
        attempt(&f, participant, id, "question", false).await;
    }
    let (task, pending) = f.seed("coding_challenge").await;
    execute(&f, "INSERT INTO challenges_course_tasks(task_id,course_id,lecture_id) VALUES($1,'course-c','lecture-3')", vec![task.into()]).await;
    attempt(&f, user, pending, "coding_challenge", false).await;
    let app = app(&f);
    let token = token(&f);
    let binding = json!({"course_id":"course-a","lecture_id":"lecture-1"});
    let before = snapshot(&f).await;
    assert_eq!(
        call(
            &app,
            user,
            Some(&token),
            json!({"lecture_bindings":[
                binding, binding,
                {"course_id":"course-a","lecture_id":"lecture-2"},
                {"course_id":"course-b","lecture_id":"lecture-1"},
                {"course_id":"course-c","lecture_id":"lecture-3"},
                {"course_id":"course-a","lecture_id":"unknown"}
            ]})
        )
        .await,
        (
            200,
            json!({"attempted_subtask_ids":[],"attempted_lecture_bindings":[
                binding, {"course_id":"course-c","lecture_id":"lecture-3"}
            ]})
        )
    );
    // A directly requested subtask must not add its unrequested lecture.
    assert_eq!(
        call(
            &app,
            user,
            Some(&token),
            json!({"subtask_ids":[pending],"lecture_bindings":[binding]})
        )
        .await,
        (
            200,
            json!({"attempted_subtask_ids":[pending],"attempted_lecture_bindings":[binding]})
        )
    );
    assert_eq!(snapshot(&f).await, before);
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn learning_history_authentication_and_batch_validation_postgres() {
    let mut f = Fixture::new().await;
    Arc::get_mut(&mut f.state).unwrap().internal_jwt_secrets = InternalJwtSecrets::new(
        f.state.jwt_secret.clone(),
        &HashMap::from([(
            "challenges".into(),
            "dedicated-synthetic-history-secret".into(),
        )]),
    )
    .unwrap();
    let app = app(&f);
    let user = Uuid::new_v4();
    let body = json!({"subtask_ids":[Uuid::new_v4()]});
    let secret = f.state.internal_jwt_secrets.get("challenges");
    let wrong_secret = JwtSecret::try_from("wrong-synthetic-secret").unwrap();
    for token in [
        None,
        Some("malformed".to_owned()),
        Some(
            sign_jwt(
                InternalAuthToken {
                    aud: "skills".into(),
                },
                secret,
                Duration::from_secs(60),
            )
            .unwrap(),
        ),
        Some(
            sign_jwt(
                InternalAuthToken {
                    aud: "challenges".into(),
                },
                &wrong_secret,
                Duration::from_secs(60),
            )
            .unwrap(),
        ),
        Some(
            sign_jwt(
                InternalAuthToken {
                    aud: "challenges".into(),
                },
                &f.state.jwt_secret,
                Duration::from_secs(60),
            )
            .unwrap(),
        ),
        Some(
            sign_jwt(
                InternalAuthToken {
                    aud: "challenges".into(),
                },
                secret,
                Duration::ZERO,
            )
            .unwrap(),
        ),
    ] {
        assert_eq!(
            call(&app, user, token.as_deref(), body.clone()).await.0,
            401
        );
    }
    for admin in [false, true] {
        assert_eq!(
            f.call(
                &app,
                user,
                admin,
                Method::POST,
                &format!("/_internal/users/{user}/learning-history"),
                body.clone()
            )
            .await
            .0,
            401
        );
    }
    let token = token(&f);
    let empty = json!({"attempted_subtask_ids":[],"attempted_lecture_bindings":[]});
    assert_eq!(
        call(&app, user, Some(&token), json!({})).await,
        (200, empty.clone())
    );
    let id = Uuid::new_v4();
    let lecture = json!({"course_id":"unknown","lecture_id":"unknown"});
    for body in [
        json!({"subtask_ids":null,"lecture_bindings":null}),
        json!({"subtask_ids":vec![id;500]}),
        json!({"lecture_bindings":vec![lecture.clone();500]}),
        json!({"subtask_ids":vec![id;499],"lecture_bindings":[lecture]}),
    ] {
        assert_eq!(
            call(&app, user, Some(&token), body).await,
            (200, empty.clone())
        );
    }
    for body in [
        json!({"subtask_ids":vec![id;501]}),
        json!({"lecture_bindings":vec![lecture.clone();501]}),
        json!({"subtask_ids":vec![id;250],"lecture_bindings":vec![lecture;251]}),
        json!({"subtask_ids":["invalid-uuid"]}),
        json!({"subtask_ids":"not-an-array"}),
        json!({"lecture_bindings":[{"course_id":"missing-lecture"}]}),
    ] {
        let (status, response) = call(&app, user, Some(&token), body).await;
        assert_eq!(status, 422, "{response}");
    }
    assert!(f.shop.lock().unwrap().access_requests.is_empty());
}
