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
        "main.c:3: error: 'ENOSPC' undeclared"))).await,
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
