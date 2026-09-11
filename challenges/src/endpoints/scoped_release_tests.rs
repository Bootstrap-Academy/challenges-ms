//! Inspect the compiled API assembled by setup_api without constructing services,
//! opening connections, invoking handlers or starting background judge workers.
use std::{collections::BTreeMap, future::Future, sync::Arc};

use lib::{config::Config, SharedState};
use poem_openapi::{registry::Registry, OpenApi};
use sandkasten_client::SandkastenClient;
use serde_json::{json, Value};

fn assembled<A, F>(_: impl FnOnce(Arc<SharedState>, Arc<Config>, SandkastenClient) -> F) -> Value
where
    A: OpenApi,
    F: Future<Output = anyhow::Result<A>>,
{
    // Infer the actual opaque tuple returned by production setup_api. Do not
    // call that factory: coding setup resumes persisted judge work.
    let mut registry = Registry::new();
    A::register(&mut registry);
    let mut operations = BTreeMap::new();
    for api in A::meta() {
        for path in api.paths {
            for operation in path.operations {
                let key = format!("{} {}", operation.method, path.path);
                assert!(operations.insert(key, json!(operation)).is_none());
            }
        }
    }
    json!({"operations": operations, "schemas": registry.schemas,
        "security_schemes": registry.security_schemes})
}

const CHARGED: [&str; 4] = [
    "/tasks/{task_id}/multiple_choice/{subtask_id}/attempts",
    "/tasks/{task_id}/matchings/{subtask_id}/attempts",
    "/tasks/{task_id}/questions/{subtask_id}/attempts",
    "/tasks/{task_id}/coding_challenges/{subtask_id}/submissions",
];

#[test]
fn scoped_release_metadata_is_the_complete_registered_surface() {
    let value = assembled(super::setup_api);
    assert!(value["operations"].as_object().unwrap().len() > 100);
    for operation in value["operations"].as_object().unwrap().values() {
        if let Some(security) = operation["security"].as_array() {
            for requirement in security {
                for name in requirement.as_object().unwrap().keys() {
                    assert!(!value["security_schemes"][name].is_null());
                }
            }
        }
    }
    if let Some(path) = std::env::var_os("SCOPED_RELEASE_METADATA") {
        std::fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    }
}

#[test]
fn scoped_release_charged_posts_are_absent() {
    let value = assembled(super::setup_api);
    for path in CHARGED {
        let key = format!("POST /learning{path}");
        assert!(value["operations"][&key].is_null(), "still exposed: {key}");
    }
    let scoped_writes: Vec<_> = value["operations"]
        .as_object()
        .unwrap()
        .keys()
        .filter(|key| key.contains(" /learning/") && !key.starts_with("GET "))
        .map(String::as_str)
        .collect();
    assert_eq!(scoped_writes, ["POST /learning/tasks/{task_id}/coding_challenges/{subtask_id}/examples/{example_id}/test"]);
}

#[test]
fn scoped_release_ordinary_writes_and_scoped_reads_keep_their_authentication() {
    let value = assembled(super::setup_api);
    let auth = |key: &str, scheme: &str| {
        let operation = &value["operations"][key];
        assert!(!operation.is_null(), "missing: {key}");
        assert_eq!(operation["security"], json!([{scheme: []}]), "{key}");
    };
    for path in CHARGED {
        auth(&format!("POST {path}"), "VerifiedUserAuth");
    }
    for family in [
        "multiple_choice",
        "matchings",
        "questions",
        "coding_challenges",
    ] {
        auth(
            &format!("GET /learning/tasks/{{task_id}}/{family}"),
            "LearningAuth",
        );
        auth(
            &format!("GET /learning/tasks/{{task_id}}/{family}/{{subtask_id}}"),
            "LearningAuth",
        );
    }
    auth(
        "GET /learning/tasks/{task_id}/coding_challenges/{subtask_id}/submissions",
        "LearningAuth",
    );
    auth(
        "GET /learning/tasks/{task_id}/coding_challenges/{subtask_id}/submissions/{submission_id}",
        "LearningAuth",
    );
    auth(
        "POST /learning/tasks/{task_id}/coding_challenges/{subtask_id}/examples/{example_id}/test",
        "LearningAuth",
    );
    assert_eq!(
        value["security_schemes"]["LearningAuth"]["name"],
        "x-learning-key"
    );
    assert_eq!(value["security_schemes"]["LearningAuth"]["type"], "apiKey");
    assert_eq!(
        value["security_schemes"]["VerifiedUserAuth"]["scheme"],
        "bearer"
    );
}
