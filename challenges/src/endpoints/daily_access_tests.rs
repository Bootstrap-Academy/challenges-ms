//! Native persistence with authenticated routes and local service authorities.
//! Skills is a contract stub here; its counter/concurrency tests belong there.
use fnct::format::JsonFormatter;
use poem::{http::Method, Endpoint, EndpointExt, Route};
use poem_ext::db::DbTransactionMiddleware;
use poem_openapi::OpenApiService;
use sandkasten_client::SandkastenClient;
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::{json, Value};
use uuid::Uuid;

use super::heart_tests::Fixture;

async fn app(f: &Fixture) -> impl Endpoint {
    Route::new()
        .nest(
            "/",
            OpenApiService::new(
                (
                    super::multiple_choice::MultipleChoice {
                        state: f.state.clone(),
                        config: f.config.clone(),
                    },
                    super::matchings::Matchings {
                        state: f.state.clone(),
                        config: f.config.clone(),
                    },
                    super::question::Questions {
                        state: f.state.clone(),
                        config: f.config.clone(),
                    },
                    super::course_tasks::CourseTasks {
                        state: f.state.clone(),
                    },
                    super::subtasks::Subtasks {
                        state: f.state.clone(),
                        config: f.config.clone(),
                    },
                    super::coding_challenges::CodingChallenges {
                        state: f.state.clone(),
                        config: f.config.clone(),
                        sandkasten: SandkastenClient::new(
                            f.config.challenges.coding_challenges.sandkasten_url.clone(),
                        ),
                        judge_cache: f.state.cache.with_formatter(JsonFormatter),
                    }
                    .setup_api()
                    .await
                    .unwrap(),
                ),
                "Daily learning regression",
                "1",
            ),
        )
        .with(DbTransactionMiddleware::new(f.state.db.clone()))
        .with(crate::services::hearts::SettlementMiddleware(
            f.state.clone(),
        ))
        .data(f.state.clone())
}

async fn count(f: &Fixture, table: &str, column: &str, user: Uuid) -> i64 {
    f.state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            format!("SELECT count(*) AS n FROM {table} WHERE {column}=$1"),
            [user.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap()
}

async fn bind(f: &Fixture, task: Uuid, course: &str, lecture: Option<&str>) {
    f.state.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO challenges_course_tasks(task_id,course_id,section_id,lecture_id) VALUES($1,$2,'section',$3)",
        [task.into(), course.into(), lecture.into()])).await.unwrap();
}

fn daily(f: &Fixture, user: Uuid) {
    let mut shop = f.shop.lock().unwrap();
    shop.modes.insert(user, "daily".into());
    shop.balances.insert(user, 0);
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn daily_quiz_results_retries_and_quota_denial_postgres() {
    let f = Fixture::new().await;
    let app = app(&f).await;
    for (kind, route, wrong, correct, table) in [
        (
            "multiple_choice_question",
            "multiple_choice",
            json!({"answers":[false,true]}),
            json!({"answers":[true,false]}),
            "challenges_multiple_choice_attempts",
        ),
        (
            "matching",
            "matchings",
            json!({"answer":[1,0]}),
            json!({"answer":[0,1]}),
            "challenges_matching_attempts",
        ),
        (
            "question",
            "questions",
            json!({"answer":"no"}),
            json!({"answer":"yes"}),
            "challenges_question_attempts",
        ),
    ] {
        let user = Uuid::new_v4();
        let (task, subtask) = f.seed(kind).await;
        bind(&f, task, "synthetic-course", Some("lecture")).await;
        f.state
            .db
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "UPDATE challenges_subtasks SET xp=9 WHERE id=$1",
                [subtask.into()],
            ))
            .await
            .unwrap();
        daily(&f, user);
        f.shop.lock().unwrap().deny_new_starts.insert(user);
        let base = format!("/tasks/{task}/{route}/{subtask}");
        assert_eq!(
            f.call(&app, user, false, Method::GET, &base, json!(null))
                .await
                .0,
            200
        );
        assert!(!f.shop.lock().unwrap().started.contains(&(user, subtask)));
        let (status, body) = f
            .call(
                &app,
                user,
                false,
                Method::POST,
                &format!("{base}/attempts"),
                wrong.clone(),
            )
            .await;
        assert_eq!(status, 429, "{body}");
        assert_eq!(body["code"], "daily_limit_reached");
        assert_eq!(body["daily"]["remaining"], 0);
        assert_eq!(count(&f, table, "user_id", user).await, 0);
        assert_eq!(
            count(&f, "challenges_user_subtasks", "user_id", user).await,
            0
        );
        f.shop.lock().unwrap().deny_new_starts.remove(&user);
        let mut ids = std::collections::HashSet::new();
        for (answer, solved) in [
            (wrong.clone(), false),
            (correct.clone(), true),
            (correct, true),
            (wrong, false),
        ] {
            let (status, body) = f
                .call(
                    &app,
                    user,
                    false,
                    Method::POST,
                    &format!("{base}/attempts"),
                    answer,
                )
                .await;
            assert_eq!(status, 201, "{body}");
            assert_eq!(body["solved"], solved);
            assert_eq!(body["hearts_pending"], false);
            assert!(ids.insert(body["attempt_id"].clone().to_string()));
            let mut shop = f.shop.lock().unwrap();
            assert_eq!(shop.balances[&user], 0);
            assert_eq!(
                shop.access_requests.last().unwrap().2["request_id"],
                body["attempt_id"]
            );
            // Once admitted, future attempts remain possible at the daily cap.
            shop.deny_new_starts.insert(user);
        }
        assert_eq!(count(&f, table, "user_id", user).await, 4);
        assert_eq!(
            count(&f, "challenge_benefit_earnings", "user_id", user).await,
            1
        );
        let rewards = f
            .state
            .db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT kind,request FROM challenge_benefit_components WHERE user_id=$1",
                [user.into()],
            ))
            .await
            .unwrap();
        assert_eq!(rewards.len(), 1);
        assert_eq!(rewards[0].try_get::<String>("", "kind").unwrap(), "xp");
        let reward: Value = rewards[0].try_get("", "request").unwrap();
        assert_eq!(reward["xp"], 9);
        assert_eq!(reward["skill_id"], "synthetic-skill");

        assert_eq!(
            count(&f, "challenge_heart_operations", "user_id", user).await,
            0
        );
    }
    assert_eq!(f.shop.lock().unwrap().heart_reads, 0);
    assert_eq!(f.shop.lock().unwrap().calls, 0);
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn daily_course_reads_and_direct_ids_postgres() {
    let f = Fixture::new().await;
    let app = app(&f).await;
    let user = Uuid::new_v4();
    daily(&f, user);
    let (task, subtask) = f.seed("multiple_choice_question").await;
    bind(&f, task, "locked-course", Some("lecture")).await;
    f.shop
        .lock()
        .unwrap()
        .deny_courses
        .insert("locked-course".into());
    let base = format!("/tasks/{task}/multiple_choice/{subtask}");
    for path in [format!("/courses/locked-course/tasks/{task}"), base.clone()] {
        assert_eq!(
            f.call(&app, user, false, Method::GET, &path, json!(null))
                .await
                .0,
            404
        );
    }
    for path in [
        "/courses/locked-course/tasks".to_owned(),
        format!("/tasks/{task}/multiple_choice"),
        format!("/subtasks?task_id={task}"),
        "/skills/synthetic-skill/tasks".into(),
    ] {
        let (status, body) = f
            .call(&app, user, false, Method::GET, &path, json!(null))
            .await;
        assert_eq!(status, 200, "{body}");
        if path.starts_with("/skills/") {
            assert!(body
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["course_id"] != "locked-course"));
        } else {
            assert_eq!(body, json!([]));
        }
    }
    let (_, stats) = f
        .call(
            &app,
            user,
            false,
            Method::GET,
            &format!("/subtasks/stats?task_id={task}"),
            json!(null),
        )
        .await;
    assert_eq!(stats["total"], 0);
    let (status, _) = f
        .call(
            &app,
            user,
            false,
            Method::POST,
            &format!("{base}/attempts?course_id=free"),
            json!({"answers":[true,false]}),
        )
        .await;
    assert_eq!(status, 403);
    assert_eq!(
        count(&f, "challenges_multiple_choice_attempts", "user_id", user).await,
        0
    );
    let (_, _, outbound) = f
        .shop
        .lock()
        .unwrap()
        .access_requests
        .last()
        .unwrap()
        .clone();
    assert_eq!(
        outbound["lecture_bindings"],
        json!([{"course_id":"locked-course","section_id":"section","lecture_id":"lecture"}])
    );
    // An unrelated standalone exercise is not blocked by a private course.
    let (public_task, public_subtask) = f.seed("question").await;
    assert_eq!(
        f.call(
            &app,
            user,
            false,
            Method::POST,
            &format!("/tasks/{public_task}/questions/{public_subtask}/attempts"),
            json!({"answer":"yes"})
        )
        .await
        .0,
        201
    );
    assert_eq!(
        f.shop.lock().unwrap().access_requests.last().unwrap().2["lecture_bindings"],
        json!([])
    );
    // A catalogue-only binding is still checked even without a CourseTask row.
    f.shop.lock().unwrap().deny_subtasks.insert(public_subtask);
    assert_eq!(
        f.call(
            &app,
            user,
            false,
            Method::POST,
            &format!("/tasks/{public_task}/questions/{public_subtask}/attempts"),
            json!({"answer":"yes"})
        )
        .await
        .0,
        403
    );
    assert_eq!(
        f.call(
            &app,
            user,
            false,
            Method::GET,
            &format!("/tasks/{public_task}/questions/{public_subtask}"),
            json!(null)
        )
        .await
        .0,
        404
    );
    f.shop.lock().unwrap().deny_subtasks.clear();
    f.shop.lock().unwrap().deny_courses.clear();
    f.shop.lock().unwrap().deny_new_starts.insert(user);
    for path in [
        "/courses/locked-course/tasks".to_owned(),
        format!("/courses/locked-course/tasks/{task}"),
        base.clone(),
    ] {
        assert_eq!(
            f.call(&app, user, false, Method::GET, &path, json!(null))
                .await
                .0,
            200
        );
    }
    assert!(!f.shop.lock().unwrap().started.contains(&(user, subtask)));
    // Course/section-wide tasks retain a course binding even without a lecture.
    let (wide, _) = f.seed("question").await;
    bind(&f, wide, "synthetic-course", None).await;
    assert_eq!(
        f.call(
            &app,
            user,
            false,
            Method::GET,
            &format!("/courses/synthetic-course/tasks/{wide}"),
            json!(null)
        )
        .await
        .0,
        200
    );
    let request = f
        .shop
        .lock()
        .unwrap()
        .access_requests
        .last()
        .unwrap()
        .2
        .clone();
    assert_eq!(request["task_id"], json!(wide));
    assert!(request["subtask_id"].is_null());
    assert_eq!(
        request["lecture_bindings"][0]["course_id"],
        "synthetic-course"
    );
    assert!(request["lecture_bindings"][0]["lecture_id"].is_null());
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn daily_policy_failures_and_legacy_shadow_postgres() {
    let f = Fixture::new().await;
    let app = app(&f).await;
    let (task, subtask) = f.seed("question").await;
    let path = format!("/tasks/{task}/questions/{subtask}/attempts");
    for mode in ["legacy", "shadow"] {
        let user = Uuid::new_v4();
        f.shop.lock().unwrap().modes.insert(user, mode.into());
        f.shop.lock().unwrap().balances.insert(user, 0);
        assert_eq!(
            f.call(
                &app,
                user,
                false,
                Method::POST,
                &path,
                json!({"answer":"yes"})
            )
            .await
            .0,
            403
        );
        f.shop.lock().unwrap().balances.insert(user, 2);
        assert_eq!(
            f.call(
                &app,
                user,
                false,
                Method::POST,
                &path,
                json!({"answer":"wrong"})
            )
            .await
            .0,
            201
        );
        assert_eq!(f.shop.lock().unwrap().balances[&user], 0);
        assert_eq!(
            count(&f, "challenge_heart_operations", "user_id", user).await,
            1
        );
        f.shop.lock().unwrap().premium.insert(user);
        assert_eq!(
            f.call(
                &app,
                user,
                false,
                Method::POST,
                &path,
                json!({"answer":"yes"})
            )
            .await
            .0,
            201
        );
    }
    let user = Uuid::new_v4();
    for status in [401, 403, 404, 503] {
        f.shop.lock().unwrap().policy_status = Some(status);
        assert_eq!(
            f.call(
                &app,
                user,
                false,
                Method::POST,
                &path,
                json!({"answer":"yes"})
            )
            .await
            .0,
            if status == 503 { 503 } else { 500 }
        );
    }
    f.shop.lock().unwrap().policy_status = None;
    for body in [
        json!({"mode":"unexpected","premium":false}),
        json!({"mode":"daily"}),
        json!({"mode":"daily","premium":"false"}),
    ] {
        f.shop.lock().unwrap().policy_body = Some(body);
        assert_eq!(
            f.call(
                &app,
                user,
                false,
                Method::POST,
                &path,
                json!({"answer":"yes"})
            )
            .await
            .0,
            500
        );
    }
    f.shop.lock().unwrap().policy_body = None;
    daily(&f, user);
    for status in [401, 500, 503] {
        f.shop.lock().unwrap().access_status = Some(status);
        assert_eq!(
            f.call(
                &app,
                user,
                false,
                Method::POST,
                &path,
                json!({"answer":"yes"})
            )
            .await
            .0,
            if status == 401 { 500 } else { 503 }
        );
        assert_eq!(
            f.call(
                &app,
                user,
                false,
                Method::GET,
                &format!("/tasks/{task}/questions"),
                json!(null)
            )
            .await
            .0,
            500
        );
    }
    assert_eq!(
        count(&f, "challenges_question_attempts", "user_id", user).await,
        0
    );
    assert_eq!(
        count(&f, "challenge_heart_operations", "user_id", user).await,
        0
    );
    assert_eq!(
        count(&f, "challenge_benefit_earnings", "user_id", user).await,
        0
    );
    // A known administrative exception does not depend on either provider.
    f.shop.lock().unwrap().policy_status = Some(503);
    assert_eq!(
        f.call(
            &app,
            user,
            true,
            Method::POST,
            &path,
            json!({"answer":"yes"})
        )
        .await
        .0,
        201
    );
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn daily_outbox_transition_and_exact_replay_postgres() {
    let f = Fixture::new().await;
    let app = f.app();
    let (task, subtask) = f.seed("question").await;
    let path = format!("/tasks/{task}/questions/{subtask}/attempts");
    let user = Uuid::new_v4();
    f.shop.lock().unwrap().heart_status = Some(503);
    let (status, body) = f
        .call(
            &app,
            user,
            false,
            Method::POST,
            &path,
            json!({"answer":"wrong"}),
        )
        .await;
    assert_eq!(status, 201);
    assert_eq!(body["hearts_pending"], true);
    let operation: Uuid = body["attempt_id"].as_str().unwrap().parse().unwrap();
    {
        let mut shop = f.shop.lock().unwrap();
        shop.modes.insert(user, "daily".into());
        shop.heart_status = None;
        shop.malformed_receipt = true;
    }
    assert!(
        !crate::services::hearts::settle(&f.state.db, &f.state.services, operation)
            .await
            .unwrap()
    );
    f.shop.lock().unwrap().malformed_receipt = false;
    let (a, b) = tokio::join!(
        crate::services::hearts::settle(&f.state.db, &f.state.services, operation),
        crate::services::hearts::settle(&f.state.db, &f.state.services, operation),
    );
    assert!(a.unwrap() && b.unwrap());
    let row = f
        .state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT state,receipt FROM challenge_heart_operations WHERE id=$1",
            [operation.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<String>("", "state").unwrap(), "settled");
    let receipt: Value = row.try_get("", "receipt").unwrap();
    assert_eq!(receipt["outcome"], "daily_learning");
    assert_eq!(receipt["charged_half_hearts"], 0);
    assert_eq!(receipt["hearts"], 6);
    f.shop.lock().unwrap().modes.insert(user, "legacy".into());
    let calls = f.shop.lock().unwrap().calls;
    assert!(
        crate::services::hearts::settle(&f.state.db, &f.state.services, operation)
            .await
            .unwrap()
    );
    assert_eq!(f.shop.lock().unwrap().calls, calls);
    assert_eq!(f.shop.lock().unwrap().balances[&user], 6);
    // A charged receipt lost in transit stays the exact old result on replay.
    let previous = Uuid::new_v4();
    f.shop.lock().unwrap().lose_reply = true;
    let (_, body) = f
        .call(
            &app,
            previous,
            false,
            Method::POST,
            &path,
            json!({"answer":"wrong"}),
        )
        .await;
    let old: Uuid = body["attempt_id"].as_str().unwrap().parse().unwrap();
    assert_eq!(body["hearts_pending"], true);
    f.shop
        .lock()
        .unwrap()
        .modes
        .insert(previous, "daily".into());
    assert!(
        crate::services::hearts::settle(&f.state.db, &f.state.services, old)
            .await
            .unwrap()
    );
    assert_eq!(f.shop.lock().unwrap().receipts[&old]["outcome"], "charged");
    assert_eq!(f.shop.lock().unwrap().balances[&previous], 4);
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn daily_coding_submission_test_and_private_code_postgres() {
    let f = Fixture::new().await;
    let app = app(&f).await;
    let user = Uuid::new_v4();
    daily(&f, user);
    let (task, subtask) = f.seed("coding_challenge").await;
    bind(&f, task, "locked-course", Some("lecture")).await;
    let base = format!("/tasks/{task}/coding_challenges/{subtask}");
    let source = json!({"environment":"python","code":"print(42)"});
    f.shop
        .lock()
        .unwrap()
        .deny_courses
        .insert("locked-course".into());
    assert_eq!(
        f.call(
            &app,
            user,
            false,
            Method::POST,
            &format!("{base}/submissions"),
            source.clone()
        )
        .await
        .0,
        403
    );
    assert_eq!(
        f.call(
            &app,
            user,
            false,
            Method::POST,
            &format!("{base}/examples/example/test"),
            source.clone()
        )
        .await
        .0,
        404
    );
    f.shop.lock().unwrap().deny_courses.clear();
    f.shop.lock().unwrap().deny_new_starts.insert(user);
    let (status, body) = f
        .call(
            &app,
            user,
            false,
            Method::POST,
            &format!("{base}/submissions"),
            source.clone(),
        )
        .await;
    assert_eq!(status, 429, "{body}");
    assert_eq!(body["code"], "daily_limit_reached");
    let (status, body) = f
        .call(
            &app,
            user,
            false,
            Method::POST,
            &format!("{base}/examples/example/test"),
            source.clone(),
        )
        .await;
    assert_eq!(status, 429, "{body}");
    assert_eq!(body["code"], "daily_limit_reached");
    assert_eq!(f.shop.lock().unwrap().learner_executions, 0);
    assert_eq!(
        count(
            &f,
            "challenges_coding_challenge_submissions",
            "creator",
            user
        )
        .await,
        0
    );
    f.shop.lock().unwrap().deny_new_starts.remove(&user);
    let (status, result) = f
        .call(
            &app,
            user,
            false,
            Method::POST,
            &format!("{base}/examples/example/test"),
            source.clone(),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(f.shop.lock().unwrap().learner_executions, 1);
    // A retry of already-started work stays free even when new starts are capped.
    f.shop.lock().unwrap().deny_new_starts.insert(user);
    let (status, body) = f
        .call(
            &app,
            user,
            false,
            Method::POST,
            &format!("{base}/submissions"),
            source.clone(),
        )
        .await;
    assert_eq!(status, 201, "{body}");
    let submission: Uuid = body["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        f.shop.lock().unwrap().access_requests.last().unwrap().2["request_id"],
        json!(submission)
    );
    let row = f
        .state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT charge_on_failure FROM challenges_coding_challenge_submissions WHERE id=$1",
            [submission.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    assert!(!row.try_get::<bool>("", "charge_on_failure").unwrap());
    let path = format!("{base}/submissions/{submission}");
    assert_eq!(
        f.call(&app, user, false, Method::GET, &path, json!(null))
            .await,
        (200, source)
    );
    assert_eq!(
        f.call(&app, Uuid::new_v4(), false, Method::GET, &path, json!(null))
            .await
            .0,
        404
    );
    f.shop
        .lock()
        .unwrap()
        .deny_courses
        .insert("locked-course".into());
    assert_eq!(
        f.call(&app, user, false, Method::GET, &path, json!(null))
            .await
            .0,
        404
    );
    assert_eq!(
        f.call(
            &app,
            user,
            false,
            Method::GET,
            &format!("{base}/submissions"),
            json!(null)
        )
        .await
        .0,
        404
    );
    assert_eq!(
        count(&f, "challenge_heart_operations", "user_id", user).await,
        0
    );
    assert_eq!(f.shop.lock().unwrap().heart_reads, 0);
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn policy_outage_requires_trusted_daily_continuation_for_actual_answers_postgres() {
    let f = Fixture::new().await;
    let app = app(&f).await;
    for (kind, route, answer, table) in [
        (
            "multiple_choice_question",
            "multiple_choice",
            json!({"answers":[false,true]}),
            "challenges_multiple_choice_attempts",
        ),
        (
            "matching",
            "matchings",
            json!({"answer":[1,0]}),
            "challenges_matching_attempts",
        ),
        (
            "question",
            "questions",
            json!({"answer":"wrong"}),
            "challenges_question_attempts",
        ),
    ] {
        let user = Uuid::new_v4();
        let (task, subtask) = f.seed(kind).await;
        bind(&f, task, "synthetic-course", Some("lecture")).await;
        let path = format!("/tasks/{task}/{route}/{subtask}/attempts");
        {
            let mut peer = f.shop.lock().unwrap();
            peer.policy_status = Some(503);
            peer.balances.insert(user, 0);
            peer.started.insert((user, subtask));
        }
        // Generic admission, legacy, malformed policy and a forged caller
        // assertion never establish a billing exemption.
        for policy in [None, Some("legacy"), Some("invalid")] {
            {
                let mut peer = f.shop.lock().unwrap();
                if let Some(policy) = policy {
                    peer.heart_policies.insert((user, subtask), policy.into());
                } else {
                    peer.heart_policies.remove(&(user, subtask));
                }
            }
            let (status, body) = f
                .call(&app, user, false, Method::POST, &path, answer.clone())
                .await;
            assert_eq!(status, 503, "{body}");
            assert_eq!(body["code"], "learning_access_unavailable");
            assert_eq!(count(&f, table, "user_id", user).await, 0);
        }
        f.shop
            .lock()
            .unwrap()
            .heart_policies
            .insert((user, subtask), "daily".into());
        for _ in 0..2 {
            let (status, body) = f
                .call(&app, user, false, Method::POST, &path, answer.clone())
                .await;
            assert_eq!(status, 201, "{body}");
            assert_eq!(body["solved"], false);
            assert_eq!(body["hearts_pending"], false);
        }
        assert_eq!(count(&f, table, "user_id", user).await, 2);
        assert_eq!(
            count(&f, "challenge_heart_operations", "user_id", user).await,
            0
        );
        assert_eq!(
            count(&f, "challenge_benefit_earnings", "user_id", user).await,
            0
        );
        {
            let peer = f.shop.lock().unwrap();
            assert_eq!(peer.balances[&user], 0);
            assert_eq!(peer.heart_reads, 0);
            let calls: Vec<_> = peer
                .access_requests
                .iter()
                .filter(|(id, _, _)| *id == user)
                .collect();
            assert_eq!(
                calls
                    .iter()
                    .filter(|(_, action, _)| action == "start")
                    .count(),
                2
            );
            assert!(calls
                .iter()
                .all(|(_, _, request)| request["subtask_id"] == json!(subtask)));
        }

        // Another user/new lesson has no trusted prior daily admission, even
        // if the caller attaches an unrecognized billing field.
        let fresh = Uuid::new_v4();
        let mut forged = answer.clone();
        forged["heart_policy"] = json!("daily");
        assert_eq!(
            f.call(&app, fresh, false, Method::POST, &path, forged)
                .await
                .0,
            503
        );
        assert_eq!(count(&f, table, "user_id", fresh).await, 0);
    }
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn policy_outage_coding_continuation_retains_no_debit_and_admission_postgres() {
    let f = Fixture::new().await;
    let app = app(&f).await;
    let user = Uuid::new_v4();
    let (task, subtask) = f.seed("coding_challenge").await;
    bind(&f, task, "synthetic-course", Some("lecture")).await;
    let base = format!("/tasks/{task}/coding_challenges/{subtask}");
    let source = json!({"environment":"python","code":"print(42)"});
    {
        let mut peer = f.shop.lock().unwrap();
        peer.policy_status = Some(503);
        peer.balances.insert(user, 0);
    }
    for tail in ["examples/example/test", "submissions"] {
        assert_eq!(
            f.call(
                &app,
                user,
                false,
                Method::POST,
                &format!("{base}/{tail}"),
                source.clone()
            )
            .await
            .0,
            503
        );
    }
    assert_eq!(
        count(
            &f,
            "challenges_coding_challenge_submissions",
            "creator",
            user
        )
        .await,
        0
    );
    assert_eq!(f.shop.lock().unwrap().learner_executions, 0);
    {
        let mut peer = f.shop.lock().unwrap();
        peer.heart_policies.insert((user, subtask), "daily".into());
        peer.started.insert((user, subtask));
        peer.deny_new_starts.insert(user);
    }
    assert_eq!(
        f.call(
            &app,
            user,
            false,
            Method::POST,
            &format!("{base}/examples/example/test"),
            source.clone()
        )
        .await
        .0,
        200
    );
    let (status, body) = f
        .call(
            &app,
            user,
            false,
            Method::POST,
            &format!("{base}/submissions"),
            source,
        )
        .await;
    assert_eq!(status, 201, "{body}");
    let id: Uuid = body["id"].as_str().unwrap().parse().unwrap();
    let row = f
        .state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT charge_on_failure FROM challenges_coding_challenge_submissions WHERE id=$1",
            [id.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    assert!(!row.try_get::<bool>("", "charge_on_failure").unwrap());
    assert_eq!(
        count(&f, "challenge_heart_operations", "user_id", user).await,
        0
    );
    let peer = f.shop.lock().unwrap();
    assert_eq!(peer.heart_reads, 0);
    assert_eq!(peer.balances[&user], 0);
    assert_eq!(
        peer.access_requests.last().unwrap().2["request_id"],
        json!(id)
    );
}
