//! Real authenticated HTTP paths with disposable PostgreSQL/Redis and local Auth.
use poem::{http::Method, EndpointExt, Route};
use poem_ext::{db::DbTransactionMiddleware, patch_value::PatchValue};
use poem_openapi::OpenApiService;
use schemas::challenges::subtasks::{CreateSubtaskRequest, UpdateSubtaskRequest};
use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use super::heart_tests::Fixture;

fn content_reference(value: &Value, author: Uuid, solved: bool) {
    assert_eq!(value["creator"], author.to_string());
    assert_eq!(value["solved"], solved);
    for field in [
        "user",
        "profile",
        "display_name",
        "avatar_url",
        "email",
        "bio",
        "tags",
        "total_xp",
        "public_rank",
        "profile_url",
        "scope_version",
    ] {
        assert!(value.get(field).is_none(), "personal enrichment: {field}");
    }
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn private_content_author_references_postgres() {
    for enabled in [false, true] {
        let mut f = Fixture::new().await;
        Arc::get_mut(&mut f.config)
            .unwrap()
            .challenges
            .profile_publications_enabled = enabled;
        let app = f.app();
        let learner = Uuid::new_v4();
        // Auth exposes no publication permission for any author. Any attempt
        // to turn their content reference into a profile lookup fails the stub.
        for (kind, route, answer) in [
            (
                "multiple_choice_question",
                "multiple_choice",
                json!({"answers":[true,false]}),
            ),
            ("matching", "matchings", json!({"answer":[0,1]})),
            ("question", "questions", json!({"answer":"yes"})),
        ] {
            let (task, subtask) = f.seed(kind).await;
            let author: Uuid = f
                .state
                .db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "SELECT creator FROM challenges_subtasks WHERE id=$1",
                    [subtask.into()],
                ))
                .await
                .unwrap()
                .unwrap()
                .try_get("", "creator")
                .unwrap();
            f.state.db.execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "INSERT INTO challenges_user_subtasks(user_id,subtask_id,solved_timestamp,attempts) VALUES($1,$2,now(),1)",
                [author.into(), subtask.into()],
            )).await.unwrap();
            let list = format!("/tasks/{task}/{route}?creator={author}");
            let detail = format!("/tasks/{task}/{route}/{subtask}");
            let (status, rows) = f
                .call(&app, learner, false, Method::GET, &list, json!({}))
                .await;
            assert_eq!(status, 200, "{rows}");
            assert_eq!(rows.as_array().unwrap().len(), 1);
            content_reference(&rows[0], author, false);
            let (status, value) = f
                .call(&app, learner, false, Method::GET, &detail, json!({}))
                .await;
            assert_eq!(status, 200, "{value}");
            content_reference(&value, author, false);
            let (status, rows) = f
                .call(
                    &app,
                    learner,
                    false,
                    Method::GET,
                    &format!("{list}&solved=true"),
                    json!({}),
                )
                .await;
            assert_eq!(status, 200, "{rows}");
            assert!(rows.as_array().unwrap().is_empty());
            // Playing the same private author's exercise records the learner's
            // own success; neither the author filter nor sharing changes it.
            let (status, result) = f
                .call(
                    &app,
                    learner,
                    false,
                    Method::POST,
                    &format!("{detail}/attempts"),
                    answer,
                )
                .await;
            assert_eq!(status, 201, "{result}");
            assert_eq!(result["solved"], true);
            let (status, rows) = f
                .call(
                    &app,
                    learner,
                    false,
                    Method::GET,
                    &format!("{list}&solved=true"),
                    json!({}),
                )
                .await;
            assert_eq!(status, 200, "{rows}");
            assert_eq!(rows.as_array().unwrap().len(), 1);
            content_reference(&rows[0], author, true);
            let (status, rows) = f
                .call(
                    &app,
                    learner,
                    false,
                    Method::GET,
                    &format!("/tasks/{task}/{route}?creator={learner}"),
                    json!({}),
                )
                .await;
            assert_eq!(status, 200, "{rows}");
            assert!(rows.as_array().unwrap().is_empty());
            // Existing ownership permits the original author to inspect a
            // disabled exercise, without publishing their learner profile.
            let (hidden_task, hidden_subtask) = f.seed_with_enabled(kind, false).await;
            let hidden_author: Uuid = f
                .state
                .db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "SELECT creator FROM challenges_subtasks WHERE id=$1",
                    [hidden_subtask.into()],
                ))
                .await
                .unwrap()
                .unwrap()
                .try_get("", "creator")
                .unwrap();
            let detail = format!("/tasks/{hidden_task}/{route}/{hidden_subtask}");
            assert_eq!(
                f.call(&app, learner, false, Method::GET, &detail, json!({}))
                    .await
                    .0,
                404
            );
            let (status, value) = f
                .call(&app, hidden_author, false, Method::GET, &detail, json!({}))
                .await;
            assert_eq!(status, 200, "{value}");
            content_reference(&value, hidden_author, false);
        }
        assert_eq!(f.shop.lock().unwrap().calls, 0);
    }
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn academy_content_authority_postgres() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let f = Fixture::new().await;
    let app = f.app();
    let extra = Route::new()
        .nest(
            "/",
            OpenApiService::new(
                (
                    super::course_tasks::CourseTasks {
                        state: f.state.clone(),
                        config: f.config.clone(),
                    },
                    super::subtasks::Subtasks {
                        state: f.state.clone(),
                        config: f.config.clone(),
                    },
                ),
                "Local content authority",
                "1",
            ),
        )
        .with(DbTransactionMiddleware::new(f.state.db.clone()))
        .data(f.state.clone());
    let (task, subtask) = f.seed("multiple_choice_question").await;
    let author: Uuid = f
        .state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT creator FROM challenges_subtasks WHERE id=$1",
            [subtask.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "creator")
        .unwrap();
    let admin = Uuid::new_v4();
    f.state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO challenges_course_tasks(task_id,course_id) VALUES($1,'synthetic-course')",
            [task.into()],
        ))
        .await
        .unwrap();
    let create = json!({"question":"Choose", "answers":[{"answer":"yes","correct":true}],"single_choice":true});
    let base = format!("/tasks/{task}/multiple_choice");
    assert_eq!(
        f.call(&app, author, false, Method::POST, &base, create.clone())
            .await
            .0,
        403
    );
    assert_eq!(
        f.call(
            &extra,
            author,
            false,
            Method::POST,
            "/courses/synthetic-course/tasks",
            json!({})
        )
        .await
        .0,
        403
    );
    assert_eq!(
        f.call(
            &app,
            author,
            false,
            Method::PATCH,
            &format!("{base}/{subtask}"),
            json!({"enabled":false})
        )
        .await
        .0,
        403
    );
    assert_eq!(
        f.call(
            &extra,
            author,
            false,
            Method::DELETE,
            &format!("/tasks/{task}/subtasks/{subtask}"),
            json!({})
        )
        .await
        .0,
        403
    );
    assert_eq!(
        f.call(
            &extra,
            admin,
            true,
            Method::DELETE,
            &format!("/tasks/{task}/subtasks/{subtask}"),
            json!({})
        )
        .await
        .0,
        403
    );
    // Being the former creator preserves read access but no publication powers.
    assert_eq!(
        f.call(
            &app,
            author,
            false,
            Method::GET,
            &format!("{base}/{subtask}/solution"),
            json!({})
        )
        .await
        .0,
        200
    );
    let (status, created) = f.call(&app, admin, true, Method::POST, &base, create).await;
    assert_eq!(status, 201, "{created}");
    assert_eq!(created["coins"], 0);
    assert_eq!(created["creator"], admin.to_string());
    // Academy staff can maintain legacy material without replacing its identity.
    let (status, updated) = f
        .call(
            &app,
            admin,
            true,
            Method::PATCH,
            &format!("{base}/{subtask}"),
            json!({"question":"Updated by Academy"}),
        )
        .await;
    assert_eq!(status, 200, "{updated}");
    assert_eq!(updated["id"], subtask.to_string());
    assert_eq!(updated["creator"], author.to_string());
    assert_eq!(
        f.call(
            &app,
            author,
            false,
            Method::GET,
            &format!("{base}/{subtask}/solution"),
            json!({})
        )
        .await
        .0,
        200
    );
    // Direct service callers cannot bypass the HTTP administrator guard.
    let tx = f.state.db.begin().await.unwrap();
    let user = lib::auth::User {
        id: author,
        admin: false,
        email_verified: true,
    };
    assert!(matches!(
        crate::services::subtasks::create_subtask(
            &tx,
            &f.config,
            &user,
            task,
            CreateSubtaskRequest {
                xp: None,
                coins: None
            },
            entity::sea_orm_active_enums::ChallengesSubtaskType::MultipleChoiceQuestion
        )
        .await
        .unwrap(),
        Err(crate::services::subtasks::CreateSubtaskError::Forbidden)
    ));
    assert!(matches!(
        crate::services::subtasks::update_subtask::<
            entity::challenges_multiple_choice_quizes::Entity,
        >(
            &tx,
            &user,
            task,
            subtask,
            UpdateSubtaskRequest {
                task_id: PatchValue::Unchanged,
                xp: PatchValue::Unchanged,
                coins: PatchValue::Unchanged,
                enabled: PatchValue::Unchanged,
                retired: PatchValue::Unchanged
            }
        )
        .await
        .unwrap(),
        Err(crate::services::subtasks::UpdateSubtaskError::Forbidden)
    ));
    tx.rollback().await.unwrap();
    assert_eq!(f.shop.lock().unwrap().calls, 0);
}
