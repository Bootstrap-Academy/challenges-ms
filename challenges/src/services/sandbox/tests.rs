use poem::{
    endpoint::make,
    listener::{Acceptor, Listener, TcpListener},
    Request, Response, Server,
};
use serde_json::{json, Value};

use super::*;

pub(crate) fn run_result(status: i32, stderr: &str) -> Value {
    json!({"status":status,"stdout":"","stderr":stderr,
        "resource_usage":{"time":10,"memory":1024},
        "limits":{"cpus":1,"time":10,"memory":128,"tmpfs":0,"filesize":1,
            "file_descriptors":32,"processes":1,"stdout_max_size":4096,
            "stderr_max_size":4096,"network":false}})
}

fn compile_error(result: Value) -> String {
    json!({"error":"compile_error","details":result}).to_string()
}

pub(crate) fn success(stdout: &str) -> Value {
    let mut run = run_result(0, "");
    run["stdout"] = stdout.into();
    json!({"program_id":uuid::Uuid::new_v4(),"ttl":60,"cached":false,
        "build":null,"run":run})
}

async fn response(status: StatusCode, body: String) -> Result<BuildRunResult, Error> {
    let acceptor = TcpListener::bind("127.0.0.1:0")
        .into_acceptor()
        .await
        .unwrap();
    let address = acceptor.local_addr()[0]
        .as_socket_addr()
        .unwrap()
        .to_owned();
    let server = tokio::spawn(async move {
        Server::new_with_acceptor(acceptor)
            .run(make(move |request: Request| {
                let body = body.clone();
                async move {
                    assert_eq!(request.uri().path(), "/run");
                    Response::builder().status(status).body(body)
                }
            }))
            .await
            .unwrap();
    });
    let client = SandboxClient::new(format!("http://{address}/").parse().unwrap());
    let result = client
        .build_and_run(&BuildRunRequest {
            build: sandkasten_client::schemas::programs::BuildRequest {
                environment: "java".into(),
                ..Default::default()
            },
            run: Default::default(),
        })
        .await;
    server.abort();
    result
}

#[tokio::test]
async fn compilation_requires_documented_http_status_and_regular_compiler_exit() {
    let body = compile_error(run_result(1, "Main.java:1: error: ';' expected"));
    assert!(matches!(
        response(StatusCode::BAD_REQUEST, body.clone()).await,
        Err(Error::Rejected(error))
            if matches!(*error, ErrorResponse::Inner(BuildRunError::CompileError(_)))
    ));
    for status in [
        StatusCode::INTERNAL_SERVER_ERROR,
        StatusCode::BAD_GATEWAY,
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::GATEWAY_TIMEOUT,
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::UNAUTHORIZED,
        StatusCode::FOUND,
    ] {
        assert!(
            matches!(response(status, body.clone()).await, Err(Error::HttpStatus(actual)) if actual == status)
        );
    }
    assert!(matches!(
        response(StatusCode::NOT_FOUND, body).await,
        Err(Error::InvalidExecution)
    ));
    for phrase in [
        "No space left on device",
        "out of memory",
        "ENOSPC",
        "error: ENOSPC",
    ] {
        let stderr = format!("Main.java:3: error: ';' expected\nSystem.out.println(\"{phrase}\")\n                                   ^\n1 error");
        assert!(
            matches!(response(StatusCode::BAD_REQUEST, compile_error(run_result(1, &stderr))).await,
            Err(Error::Rejected(error)) if matches!(*error, ErrorResponse::Inner(BuildRunError::CompileError(_))))
        );
    }
    assert!(
        matches!(response(StatusCode::BAD_REQUEST, compile_error(run_result(1,
        "code.c:3:12: error: 'ENOSPC' undeclared (first use in this function)"))).await,
        Err(Error::Rejected(error)) if matches!(*error, ErrorResponse::Inner(BuildRunError::CompileError(_))))
    );
}

#[tokio::test]
async fn host_and_compiler_failures_are_technical_without_relying_on_one_message() {
    for diagnostic in [
        "java.io.IOException: No space left on device",
        "write failed: ENOSPC (os error 28)",
        "OSError: [Errno 28] Speichermedium voll",
        "Disk quota exceeded",
        "Read-only file system",
        "Cannot allocate memory",
        "Out of memory",
        "java.lang.OutOfMemoryError: Java heap space",
        "Could not reserve enough space for object heap",
        "Internal compiler error",
        "Unable to create native thread",
        "Resource temporarily unavailable",
        "Failed to create thread",
        "error while loading shared libraries: missing.so",
        "nsjail error",
    ] {
        assert!(
            matches!(
                response(
                    StatusCode::BAD_REQUEST,
                    compile_error(run_result(1, diagnostic))
                )
                .await,
                Err(Error::CompilationUnavailable)
            ),
            "{diagnostic}"
        );
    }
    for status in [0, -9, -11, 124, 125, 126, 127, 137, 143] {
        assert!(
            matches!(
                response(
                    StatusCode::BAD_REQUEST,
                    compile_error(run_result(status, ""))
                )
                .await,
                Err(Error::CompilationUnavailable)
            ),
            "process status {status}"
        );
    }
    for (field, value) in [("time", 10000), ("memory", 128 * 1024)] {
        let mut run = run_result(1, "");
        run["resource_usage"][field] = value.into();
        assert!(
            matches!(
                response(StatusCode::BAD_REQUEST, compile_error(run)).await,
                Err(Error::CompilationUnavailable)
            ),
            "resource {field}"
        );
    }
    let mut run = run_result(1, "");
    run["limits"]["time"] = 0.into();
    assert!(matches!(
        response(StatusCode::BAD_REQUEST, compile_error(run)).await,
        Err(Error::CompilationUnavailable)
    ));
}

#[tokio::test]
async fn malformed_incomplete_and_inconsistent_replies_never_grade_a_learner() {
    for body in [
        "unavailable",
        "{",
        "null",
        "{}",
        "{\"error\":\"compile_error\"}",
    ] {
        for status in [StatusCode::OK, StatusCode::BAD_REQUEST] {
            assert!(matches!(
                response(status, body.into()).await,
                Err(Error::Transport(_))
            ));
        }
    }
    let mut incomplete = run_result(1, "syntax error");
    incomplete.as_object_mut().unwrap().remove("resource_usage");
    assert!(matches!(
        response(StatusCode::BAD_REQUEST, compile_error(incomplete)).await,
        Err(Error::Transport(_))
    ));
    let mut inconsistent = success("42");
    inconsistent["build"] = run_result(1, "compiler failed despite success envelope");
    assert!(matches!(
        response(StatusCode::OK, inconsistent.to_string()).await,
        Err(Error::InvalidExecution)
    ));
    let result = response(StatusCode::OK, success("42").to_string())
        .await
        .unwrap();
    assert_eq!(result.run.stdout, "42");
    // Learner runtime output can freely contain infrastructure phrases.
    let mut runtime = success("No space left on device");
    runtime["run"]["status"] = 1.into();
    runtime["run"]["stderr"] = "out of memory".into();
    assert!(response(StatusCode::OK, runtime.to_string()).await.is_ok());
}

#[derive(serde::Deserialize)]
struct RealReply {
    source: String,
    environment: String,
    http: u16,
    expect: String,
    body: Value,
}

/// Recorded replies of a local Sandkasten 0.2.2 with the production config
/// and limits (cgroup mode as in production, plus rlimit mode), a full
/// artifact cache and an nsjail that cannot start. `expect` records how each
/// reply was produced. Lesson limits follow the judge's request mapping.
#[tokio::test]
async fn real_sandbox_replies_keep_learner_errors_and_detect_outages() {
    let replies: Vec<RealReply> = serde_json::from_str(include_str!("real_replies.json")).unwrap();
    assert_eq!(replies.len(), 104);
    for reply in replies {
        let label = format!("{} ({})", reply.source, reply.environment);
        if reply.body["error"] == "compile_error" {
            // The judgment guard for cached results agrees with the adapter.
            let details: RunResult = serde_json::from_value(reply.body["details"].clone()).unwrap();
            assert_eq!(
                technical_compilation(&details),
                reply.expect == "technical",
                "{label}"
            );
        }
        let status = StatusCode::from_u16(reply.http).unwrap();
        let actual = match response(status, reply.body.to_string()).await {
            Err(Error::Rejected(error)) => match *error {
                ErrorResponse::Inner(BuildRunError::CompileError(_)) => "compilation_error".into(),
                other => panic!("{label}: unexpected rejection {other:?}"),
            },
            Err(_) => "technical".into(),
            Ok(output) if launcher_failure(&output.run) => "technical".into(),
            Ok(output) => {
                let limits = &output.run.limits;
                let lesson_time = (limits.time - 1) * 1000;
                crate::services::judge::run_verdict(
                    &output.run,
                    Some(lesson_time),
                    Some(limits.memory),
                )
                .map_or("Ok".into(), |verdict| format!("{verdict:?}"))
            }
        };
        assert_eq!(actual, reply.expect, "{label}");
    }
}

#[test]
fn compiler_controlled_wording_is_technical_even_at_a_learner_location() {
    let technical = |stderr: &str| {
        technical_compilation(&serde_json::from_value(run_result(1, stderr)).unwrap())
    };
    for stderr in [
        "code.c:3:1: internal compiler error: Segmentation fault",
        "error: the compiler unexpectedly panicked. this is a bug.",
        "code.java:1: error: error while writing Main: No space left on device\nclass Main {}\n^",
        "/tmp/ccA1.s: Fatal error: can't write 4 bytes to section .text of /tmp/ccB2.o: 'No space left on device'",
    ] {
        assert!(technical(stderr), "{stderr}");
    }
    for stderr in [
        "code.c:1:2: error: #error internal compiler error",
        "code.c:1:2: error: #error error while writing X: No space left on device",
        "code.cpp:1:2: error: #error No space left on device\n    1 | #error No space left on device\n      |  ^~~~~",
        "error: No space left on device\n --> code.rs:1:1",
        "error[E0277]: the compiler unexpectedly panicked\n --> code.rs:4:5",
        "  = note: No space left on device",
        "/box/code.go:2:8: invalid import path: No space left on device",
        "/tmp/code.cs(1,8): error CS1029: #error: 'No space left on device' [/tmp/tmp.csproj]",
        "code.c:4:5: error: unknown type name 'NOENOSPCX'",
        "code.c:4:5: error: 'myenospc' undeclared",
        "code.hs:1:18: error: [GHC-83865]\n    • In the first argument of ‘length’, namely ‘\"No space left on device\"’",
    ] {
        assert!(!technical(stderr), "{stderr}");
    }
}

#[tokio::test]
async fn network_failure_is_technical() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let result = SandboxClient::new(format!("http://{address}/").parse().unwrap())
        .build_and_run(&BuildRunRequest {
            build: Default::default(),
            run: Default::default(),
        })
        .await;
    assert!(matches!(result, Err(Error::Transport(_))));
}
