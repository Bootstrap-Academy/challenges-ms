//! Registers the complete API type exactly as `main` does, without any live
//! service. poem-openapi panics at startup when two different types share one
//! schema name, which build, clippy and nix do not notice.
use std::{collections::HashSet, future::Future, sync::Arc};

use crate::services::sandbox::SandboxClient as SandkastenClient;
use lib::{config::Config, SharedState};
use poem_openapi::{
    registry::{MetaApi, Registry},
    Object, OpenApi,
};

/// Takes the API type from the uncalled `setup_api` function item.
fn register_setup_api<F, Fut, T>(_: F) -> (Registry, Vec<MetaApi>)
where
    F: FnOnce(Arc<SharedState>, Arc<Config>, SandkastenClient) -> Fut,
    Fut: Future<Output = anyhow::Result<T>>,
    T: OpenApi,
{
    register::<T>()
}

fn register<T: OpenApi>() -> (Registry, Vec<MetaApi>) {
    let mut registry = Registry::new();
    T::register(&mut registry);
    (registry, T::meta())
}

#[test]
fn full_api_registers_without_name_collisions() {
    let (registry, apis) = register_setup_api(super::setup_api);

    let mut routes = HashSet::new();
    let mut operation_ids = HashSet::new();
    for path in apis.iter().flat_map(|api| &api.paths) {
        for operation in &path.operations {
            assert!(
                routes.insert((path.path.clone(), operation.method.clone())),
                "duplicate route {} {}",
                operation.method,
                path.path
            );
            if let Some(id) = operation.operation_id {
                assert!(operation_ids.insert(id), "duplicate operation id {id}");
            }
        }
    }

    for name in [
        "LessonMilestone",
        "LessonMilestoneCompletion",
        "RecordLessonMilestoneRequest",
        "RecordedLessonMilestone",
        "UserDataExport",
    ] {
        assert!(registry.schemas.contains_key(name), "missing schema {name}");
    }

    let milestone = apis
        .iter()
        .flat_map(|api| &api.paths)
        .find(|path| path.path.contains("lesson-milestones"))
        .expect("lesson milestone route");
    assert_eq!(milestone.operations.len(), 1);
    let operation = &milestone.operations[0];
    assert_eq!(operation.method, poem::http::Method::PUT);
    // Internal service token only; no user, learning or public scheme.
    assert_eq!(operation.security.len(), 1);
    assert_eq!(
        operation.security[0].keys().copied().collect::<Vec<_>>(),
        ["InternalAuth"]
    );
}

mod first {
    #[derive(poem_openapi::Object)]
    pub struct Clash {
        pub a: i32,
    }
}

mod second {
    #[derive(poem_openapi::Object)]
    pub struct Clash {
        pub b: String,
    }
}

#[derive(Object)]
struct ClashHolder {
    first: first::Clash,
    second: second::Clash,
}

struct ClashApi;

#[OpenApi]
impl ClashApi {
    #[oai(path = "/clash", method = "post")]
    async fn clash(
        &self,
        body: poem_openapi::payload::Json<ClashHolder>,
    ) -> poem_openapi::payload::PlainText<String> {
        poem_openapi::payload::PlainText(format!("{}{}", body.0.first.a, body.0.second.b))
    }
}

/// The check above would catch a collision: same name, different types.
#[test]
fn registration_detects_a_name_collision() {
    let Err(panic) = std::panic::catch_unwind(register::<ClashApi>) else {
        panic!("a name collision must fail registration");
    };
    let message = panic.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(
        message.contains("have the same OpenAPI name `Clash`"),
        "{message}"
    );
}
