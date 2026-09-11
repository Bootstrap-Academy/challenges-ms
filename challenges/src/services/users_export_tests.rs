//! Opt-in PostgreSQL regression against the real migrated, disposable schema.
//! Run with T11_TEST_DATABASE_URL and `cargo test -p challenges
//! authored_export_postgres -- --ignored --nocapture`. All inserts roll back.
use poem_openapi::types::{ParseFromJSON, ToJSON};
use schemas::challenges::user_export::UserDataExport;
use sea_orm::{ConnectionTrait, Database, DbBackend, Statement, TransactionTrait};
use serde_json::{json, Value};
use uuid::Uuid;

use super::export_user_data;

fn id(suffix: &str) -> Uuid {
    format!("00000000-0000-0000-0000-{suffix:0>12}")
        .parse()
        .unwrap()
}

fn content(export: &Value, suffix: &str) -> Value {
    export["subtasks_created"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id(suffix).to_string())
        .unwrap()["content"]
        .clone()
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL via T11_TEST_DATABASE_URL"]
async fn authored_export_postgres() {
    let db = Database::connect(std::env::var("T11_TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let txn = db.begin().await.unwrap();
    txn.execute_unprepared(include_str!("fixtures/authored_export.sql"))
        .await
        .unwrap();
    // Code contains CRLF, tabs, quotes, Unicode, literal binary escape sequences,
    // and embedded file bytes represented as base64. It must stay inert text.
    let source =
        "# 雪 🦀\r\n\tdata = b'\\x00\\xff'\nprint(\"\\\\\")\n# file: AP/+AQ==\n".repeat(4000);
    txn.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "UPDATE challenges_coding_challenges SET evaluator = $1, solution_code = $1 WHERE subtask_id = $2",
        [source.clone().into(), id("101").into()])).await.unwrap();
    let export = export_user_data(&txn, id("100")).await.unwrap();
    let serialized = export.to_json_string();
    let json: Value = serde_json::from_str(&serialized).unwrap();
    // Also exercise the discriminated OpenAPI parser rather than only its writer.
    let roundtrip = UserDataExport::parse_from_json(Some(json.clone())).unwrap();
    assert_eq!(roundtrip.to_json_string(), serialized);
    assert_eq!(json["subtasks_created"].as_array().unwrap().len(), 5);
    assert_eq!(json["tasks_created"].as_array().unwrap().len(), 2);
    assert_eq!(json["subtasks_created"][0]["id"], id("101").to_string());
    assert_eq!(json["subtasks_created"][0]["retired"], true);
    assert_eq!(json["subtasks_created"][0]["enabled"], false);
    assert_eq!(json["subtasks_created"][1]["task_id"], id("3").to_string());
    assert_eq!(
        content(&json, "101"),
        json!({
            "type": "CODING_CHALLENGE", "description": "Markdown with image data:image/png;base64,AP/+AQ==",
            "evaluator": source, "solution_code": source, "solution_environment": "python",
            "time_limit": 1234, "memory_limit": 256, "static_tests": 7, "random_tests": 13,
        })
    );
    assert_eq!(
        content(&json, "102"),
        json!({
            "type": "QUESTION", "question": "## Frage 雪\n\t\"Was?\"\r\n",
            "answers": ["Ä", "", "Ä", "line\nend"], "case_sensitive": true,
            "ascii_letters": false, "digits": true, "punctuation": false, "blocks": ["```", "雪", ""],
        })
    );
    assert_eq!(
        content(&json, "103")["correct_answers_bitmask"],
        "-9223372036854775807"
    );
    assert_eq!(
        content(&json, "103")["correct_answer_indices"],
        json!([0, 63])
    );
    assert_eq!(content(&json, "103")["answers"][63], "answer-63");
    assert_eq!(
        content(&json, "104"),
        json!({
            "type": "MATCHING", "left": ["甲", "乙", "丙"], "right": ["C", "A", "B"], "solution": [1, 2, 0],
        })
    );
    assert_eq!(content(&json, "107")["solution_code"], "");
    assert_eq!(content(&json, "107")["solution_environment"], "");
    assert_eq!(
        json["tasks_created"][0]["content"],
        json!({
            "type": "CHALLENGE", "category_id": id("9"), "skill_ids": ["python", "unicode"],
            "title": "Authored title 雪", "description": "Authored parent\n```python\nprint(\"€\")\n```",
        })
    );
    assert_eq!(json["tasks_created"][1]["content"]["course_id"], "course");
    assert!(json["tasks_created"][1]["content"]["section_id"].is_null());
    assert!(json["tasks_created"][1]["content"]["lecture_id"].is_null());
    assert_eq!(
        json["coding_challenge_submissions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        json["coding_challenge_submissions"][0]["code"],
        "MY_SUBMISSION_TO_OTHER_TASK"
    );
    assert_eq!(json["question_attempts"].as_array().unwrap().len(), 1);
    assert!(!serialized.contains("OTHER_AUTHOR_PRIVATE"));
    assert!(!serialized.contains(&id("200").to_string()));
    if let Ok(path) = std::env::var("T11_CAPTURE_JSON") {
        std::fs::write(path, &serialized).unwrap();
    }
    println!("PASS all six definitions, both ownership directions, excluded private content, >500KB exact source, signed high-bit mask, nullable location and empty legacy solution");

    for (label, mutation) in [
        ("missing owned subtype", "DELETE FROM challenges_matchings WHERE subtask_id = '00000000-0000-0000-0000-000000000104'"),
        ("mismatched owned type", "UPDATE challenges_subtasks SET ty = 'question' WHERE id = '00000000-0000-0000-0000-000000000104'"),
        ("duplicate owned subtype", "INSERT INTO challenges_matchings SELECT '00000000-0000-0000-0000-000000000103', \"left\", \"right\", solution FROM challenges_matchings"),
        ("missing owned parent definition", "DELETE FROM challenges_course_tasks WHERE task_id = '00000000-0000-0000-0000-000000000002'"),
        ("duplicate owned parent definition", "INSERT INTO challenges_course_tasks VALUES ('00000000-0000-0000-0000-000000000001', 'duplicate', NULL, NULL)"),
    ] {
        txn.execute_unprepared("SAVEPOINT broken_definition").await.unwrap();
        txn.execute_unprepared(mutation).await.unwrap();
        let error = export_user_data(&txn, id("100")).await.unwrap_err();
        assert!(error.to_string().contains("missing or inconsistent subtype"), "{label}: {error}");
        txn.execute_unprepared("ROLLBACK TO SAVEPOINT broken_definition").await.unwrap();
        println!("PASS {label}: no metadata-only success");
    }
    // Legacy invalid signed solution values and mask bits outside the answer
    // count are retained rather than truncated by public API conversion helpers.
    txn.execute_unprepared("UPDATE challenges_matchings SET solution = ARRAY[-1,32767,0]::smallint[]; UPDATE challenges_multiple_choice_quizes SET answers = ARRAY['only'], correct_answers = 9007199254740993").await.unwrap();
    let legacy = export_user_data(&txn, id("100"))
        .await
        .unwrap()
        .to_json()
        .unwrap();
    assert_eq!(content(&legacy, "104")["solution"], json!([-1, 32767, 0]));
    assert_eq!(
        content(&legacy, "103")["correct_answers_bitmask"],
        "9007199254740993"
    );
    assert_eq!(
        content(&legacy, "103")["correct_answer_indices"],
        json!([0])
    );
    println!("PASS legacy invalid indices and >2^53 mask preserved without lossy casts");
    // Broken records belonging to somebody else must not block or leak into ours.
    txn.execute_unprepared("DELETE FROM challenges_questions WHERE subtask_id = '00000000-0000-0000-0000-000000000106'").await.unwrap();
    assert!(export_user_data(&txn, id("100")).await.is_ok());
    let empty = export_user_data(&txn, id("999"))
        .await
        .unwrap()
        .to_json()
        .unwrap();
    assert!(empty
        .as_object()
        .unwrap()
        .values()
        .all(|v| v.as_array().unwrap().is_empty()));
    println!("PASS unrelated corruption isolated and unknown author empty");
    txn.rollback().await.unwrap();
    db.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires a migrated disposable PostgreSQL via T11_TEST_DATABASE_URL"]
async fn authored_export_snapshot_postgres() {
    let db = Database::connect(std::env::var("T11_TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let owner = Uuid::new_v4();
    let replacement = Uuid::new_v4();
    let task = Uuid::new_v4();
    let subtask = Uuid::new_v4();
    let seed = db.begin().await.unwrap();
    seed.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO challenges_tasks VALUES ($1, $2, '2026-09-01')",
        [task.into(), owner.into()],
    ))
    .await
    .unwrap();
    seed.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO challenges_course_tasks VALUES ($1, 'snapshot', NULL, NULL)",
        [task.into()],
    ))
    .await
    .unwrap();
    seed.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO challenges_subtasks (id, task_id, creator, creation_timestamp, xp, coins, enabled, retired, ty) VALUES ($1, $2, $3, '2026-09-01', 0, 0, true, false, 'matching')",
        [subtask.into(), task.into(), owner.into()])).await.unwrap();
    seed.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO challenges_matchings VALUES ($1, ARRAY['original'], ARRAY['right'], ARRAY[0]::smallint[])", [subtask.into()])).await.unwrap();
    seed.commit().await.unwrap();
    let snapshot = db.begin().await.unwrap();
    snapshot
        .execute_unprepared("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .await
        .unwrap();
    let before = export_user_data(&snapshot, owner)
        .await
        .unwrap()
        .to_json_string();
    // Simulate a concurrent admin/database repair of the ownership and content.
    // The public update API does not itself transfer creator IDs.
    let update = db.begin().await.unwrap();
    update
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "UPDATE challenges_subtasks SET creator = $1 WHERE id = $2",
            [replacement.into(), subtask.into()],
        ))
        .await
        .unwrap();
    update
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "UPDATE challenges_matchings SET \"left\" = ARRAY['replacement'] WHERE subtask_id = $1",
            [subtask.into()],
        ))
        .await
        .unwrap();
    update.commit().await.unwrap();
    assert_eq!(
        export_user_data(&snapshot, owner)
            .await
            .unwrap()
            .to_json_string(),
        before
    );
    snapshot.rollback().await.unwrap();
    let fresh = db.begin().await.unwrap();
    assert!(export_user_data(&fresh, owner)
        .await
        .unwrap()
        .subtasks_created
        .is_empty());
    let replacement_export = export_user_data(&fresh, replacement).await.unwrap();
    assert_eq!(replacement_export.subtasks_created.len(), 1);
    assert!(replacement_export.to_json_string().contains("replacement"));
    fresh.rollback().await.unwrap();
    // Fixture cleanup follows the same authenticated erasure marker used by
    // the service; the moderation guard correctly rejects an unmarked cascade.
    let cleanup = db.begin().await.unwrap();
    crate::services::moderation::erasure_marker(&cleanup, replacement)
        .await
        .unwrap();
    cleanup
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "DELETE FROM challenges_tasks WHERE id = $1",
            [task.into()],
        ))
        .await
        .unwrap();
    cleanup.commit().await.unwrap();
    println!("PASS read-only repeatable-read export retains original ownership and source across a committed concurrent change; next snapshot sees new owner");
    db.close().await.unwrap();
}
