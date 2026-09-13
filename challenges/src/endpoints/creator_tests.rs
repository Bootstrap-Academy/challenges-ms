//! Real authenticated HTTP paths with disposable PostgreSQL/Redis and local Auth.
use poem::{http::Method, EndpointExt, Route};
use poem_ext::{db::DbTransactionMiddleware, patch_value::PatchValue};
use poem_openapi::OpenApiService;
use schemas::challenges::subtasks::{CreateSubtaskRequest, UpdateSubtaskRequest};
use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use serde_json::json;
use uuid::Uuid;

use super::heart_tests::Fixture;

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
