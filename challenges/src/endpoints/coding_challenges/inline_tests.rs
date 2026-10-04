//! Native admission and real authenticated routes, with a synthetic HTTP
//! executor. No learner code, live account or production service is used.
use crate::services::sandbox::SandboxClient as SandkastenClient;
use std::sync::Mutex;

use fnct::format::JsonFormatter;
use lib::services::Services;
use poem::{
    endpoint::make,
    http::Method,
    listener::{Acceptor, Listener, TcpListener},
    Endpoint, EndpointExt, IntoResponse, Request, Route, Server,
};
use poem_ext::db::DbTransactionMiddleware;
use poem_openapi::OpenApiService;
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DbBackend, Statement};
use serde_json::{json, Value};

use super::*;
use crate::{
    endpoints::heart_tests::Fixture,
    services::{coding_execution as queue, users::delete_user_data},
};

#[derive(Default)]
struct ExecutorState {
    stall: Option<String>,
    fail: bool,
    reply: Option<(String, StatusCode, String)>,
    phases: Vec<String>,
    authority: Option<Uuid>,
}

struct Executor {
    url: String,
    state: Arc<Mutex<ExecutorState>>,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Executor {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Executor {
    async fn new() -> Self {
        let state = Arc::new(Mutex::new(ExecutorState::default()));
        let endpoint = make({
            let state = state.clone();
            move |mut request: Request| {
                let state = state.clone();
                async move {
                    let path = request.uri().path().to_owned();
                    let value = if path.ends_with("learning_authority_digest") {
                        let body: Value = request.take_body().into_json().await.unwrap();
                        assert_eq!(body["hash"].as_str().unwrap().len(), 64);
                        let user = state.lock().unwrap().authority.unwrap();
                        json!({"purpose":"retained_learning","ordinary_authority":false,
                            "financial_authority":false,"admin":false,"email_verified":true,
                            "subject":user})
                    } else if path.contains("learning-policy") {
                        json!({"mode":"legacy","premium":false,"single_course_sales":true,
                            "heart_sales":true})
                    } else if path.contains("hearts") {
                        json!({"hearts":6})
                    } else if path == "/environments" {
                        json!({"python":{"name":"Python","version":"synthetic",
                            "default_main_file_name":"code.py","example":null,"meta":{}}})
                    } else {
                        assert_eq!(path, "/run");
                        let body: Value = request.take_body().into_json().await.unwrap();
                        let phase = body["run"]["args"][0]
                            .as_str()
                            .unwrap_or("solution")
                            .to_owned();
                        let (stall, fail, reply) = {
                            let mut state = state.lock().unwrap();
                            state.phases.push(phase.clone());
                            (
                                state.stall.as_ref() == Some(&phase),
                                state.fail,
                                state.reply.clone(),
                            )
                        };
                        if stall {
                            tokio::time::sleep(Duration::from_secs(3)).await;
                        }
                        if fail {
                            return Response::builder()
                                .status(StatusCode::SERVICE_UNAVAILABLE)
                                .body("synthetic executor unavailable");
                        }
                        if let Some((at, status, body)) = reply {
                            if at == phase {
                                return Response::builder().status(status).body(body);
                            }
                        }
                        let stdout = match phase.as_str() {
                            "examples" => json!(["example"]).to_string(),
                            "generate" => json!({"input":"42","data":null}).to_string(),
                            "prepare" => json!({"code":"synthetic code","reason":""}).to_string(),
                            "solution" => "42".into(),
                            "check" => {
                                json!({"verdict":"WRONG_ANSWER","reason":"synthetic"}).to_string()
                            }
                            _ => panic!("unexpected synthetic executor phase {phase}"),
                        };
                        json!({"program_id":Uuid::new_v4(),"ttl":60,"cached":false,"build":null,
                            "run":{"status":0,"stdout":stdout,"stderr":"",
                                "resource_usage":{"time":1,"memory":1},
                                "limits":{"cpus":1,"time":1,"memory":128,"tmpfs":0,"filesize":1,
                                    "file_descriptors":32,"processes":1,"stdout_max_size":4096,
                                    "stderr_max_size":4096,"network":false}}})
                    };
                    Response::builder()
                        .content_type("application/json")
                        .body(value.to_string())
                }
            }
        });
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
                .run(endpoint)
                .await
                .unwrap();
        });
        Self {
            url: format!("http://{address}/"),
            state,
            server,
        }
    }

    async fn reached(&self, phase: &str) {
        self.reached_count(phase, 1).await;
    }

    async fn reached_count(&self, phase: &str, count: usize) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if self
                    .state
                    .lock()
                    .unwrap()
                    .phases
                    .iter()
                    .filter(|p| p.as_str() == phase)
                    .count()
                    >= count
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("executor phase reached");
    }
}

fn configure(f: &mut Fixture, executor: &Executor, global: u32, personal: u32, seconds: u32) {
    let config = Arc::get_mut(&mut f.config).unwrap();
    config.challenges.coding_challenges.sandkasten_url = executor.url.parse().unwrap();
    config.challenges.coding_challenges.timeout = 0;
    let settings = &mut config.challenges.coding_challenges.execution;
    settings.max_pending = global;
    settings.max_pending_per_user = personal;
    settings.max_execution_seconds = seconds;
    settings.poll_milliseconds = 10;
    if executor.state.lock().unwrap().authority.is_some() {
        config.services.shop = executor.url.parse().unwrap();
        let state = Arc::get_mut(&mut f.state).unwrap();
        state.services = Services::from_config(
            &state.internal_jwt_secrets,
            Duration::from_secs(60),
            &config.services,
            state.cache.clone(),
        );
    }
}

async fn app(f: &Fixture) -> impl Endpoint {
    Route::new()
        .nest(
            "/",
            OpenApiService::new(
                super::super::CodingChallenges {
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
                "Inline admission regression",
                "1",
            ),
        )
        .with(DbTransactionMiddleware::new(f.state.db.clone()))
        .data(f.state.clone())
}

fn solution() -> Value {
    json!({"environment":"python","code":"synthetic learner code"})
}

fn path(task: Uuid, subtask: Uuid) -> String {
    format!("/tasks/{task}/coding_challenges/{subtask}/examples/example/test")
}

async fn inline_count(f: &Fixture, user: Uuid) -> i64 {
    f.state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT count(*) AS n FROM challenge_coding_inline_runs WHERE user_id=$1",
            [user.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap()
}

async fn cleaned(f: &Fixture, user: Uuid) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while inline_count(f, user).await != 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("inline reservation released");
}

async fn no_learning_effects(f: &Fixture, user: Uuid) {
    for (table, column) in [
        ("challenges_coding_challenge_submissions", "creator"),
        ("challenges_user_subtasks", "user_id"),
        ("challenge_heart_operations", "user_id"),
        ("challenge_benefit_earnings", "user_id"),
    ] {
        let row = f
            .state
            .db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Postgres,
                format!("SELECT count(*) AS n FROM {table} WHERE {column}=$1"),
                [user.into()],
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<i64>("", "n").unwrap(), 0, "{table}");
    }
}

async fn remove_task(f: &Fixture, task: Uuid) {
    let cleanup = f.state.db.begin().await.unwrap();
    let author: Uuid = cleanup
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT creator FROM challenges_tasks WHERE id=$1",
            [task.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "creator")
        .unwrap();
    // The fixture's synthetic author owns only this task. Use normal erasure
    // so moderation withdrawal and deletion cascades stay enforced.
    delete_user_data(&cleanup, author).await.unwrap();
    cleanup.commit().await.unwrap();
}

#[tokio::test]
#[ignore = "requires fresh disposable PostgreSQL and Redis; run coding_inline tests separately"]
async fn coding_inline_and_submission_share_admission_postgres() {
    let mut f = Fixture::new().await;
    let executor = Executor::new().await;
    configure(&mut f, &executor, 5, 4, 10);
    let (task, subtask) = f.seed("coding_challenge").await;
    let user = Uuid::new_v4();
    let endpoint = app(&f).await;
    let submissions = format!("/tasks/{task}/coding_challenges/{subtask}/submissions");
    for _ in 0..4 {
        let (status, body) = f
            .call(
                &endpoint,
                user,
                false,
                Method::POST,
                &submissions,
                solution(),
            )
            .await;
        assert_eq!(status, 201, "{body}");
    }
    let (status, body) = f
        .call(
            &endpoint,
            user,
            false,
            Method::POST,
            &path(task, subtask),
            solution(),
        )
        .await;
    assert_eq!(status, 429, "{body}");
    assert_eq!(body["error"], "coding_execution_busy");
    assert!(executor.state.lock().unwrap().phases.is_empty());
    assert_eq!(inline_count(&f, user).await, 0);

    // A different subject can take the last global slot as an inline lease.
    let other = Uuid::new_v4();
    let mut lease = InlineRun::reserve(
        &f.state.db,
        other,
        subtask,
        &f.config.challenges.coding_challenges.execution,
    )
    .await
    .unwrap()
    .unwrap();
    let third = Uuid::new_v4();
    let (status, body) = f
        .call(
            &endpoint,
            third,
            false,
            Method::POST,
            &submissions,
            solution(),
        )
        .await;
    assert_eq!(status, 429, "{body}");
    let (status, body) = f
        .call(
            &endpoint,
            third,
            false,
            Method::POST,
            &path(task, subtask),
            solution(),
        )
        .await;
    assert_eq!(status, 429, "{body}");
    no_learning_effects(&f, other).await;
    no_learning_effects(&f, third).await;
    lease.release().await.unwrap();
    assert_eq!(
        f.call(
            &endpoint,
            third,
            false,
            Method::POST,
            &path(task, subtask),
            solution()
        )
        .await
        .0,
        200
    );
    no_learning_effects(&f, third).await;
    f.state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "DELETE FROM challenges_coding_challenge_submissions WHERE subtask_id=$1",
            [subtask.into()],
        ))
        .await
        .unwrap();
    // A queued submission and an inline lease race for one global slot while
    // using different transactions. Exactly one committed reservation wins.
    let settings = lib::config::CodingExecution {
        max_pending: 1,
        max_pending_per_user: 1,
        ..Default::default()
    };
    let inline_user = Uuid::new_v4();
    let queued_user = Uuid::new_v4();
    let queued = async {
        let transaction = f.state.db.begin().await.unwrap();
        crate::services::benefits::lock_attempt(&transaction, queued_user)
            .await
            .unwrap();
        let admitted = queue::admit(&transaction, queued_user, &settings)
            .await
            .unwrap();
        if admitted {
            transaction.execute(Statement::from_sql_and_values(DbBackend::Postgres,
                "INSERT INTO challenges_coding_challenge_submissions(id,subtask_id,creator,creation_timestamp,environment,code,charge_on_failure) VALUES($1,$2,$3,now(),'python','synthetic',false)",
                [Uuid::new_v4().into(), subtask.into(), queued_user.into()])).await.unwrap();
        }
        transaction.commit().await.unwrap();
        admitted
    };
    let (inline, queued) = tokio::join!(
        InlineRun::reserve(&f.state.db, inline_user, subtask, &settings),
        queued,
    );
    let mut inline = inline.unwrap();
    assert_ne!(inline.is_some(), queued);
    if let Some(guard) = inline.as_mut() {
        guard.release().await.unwrap();
    }
    no_learning_effects(&f, inline_user).await;
    remove_task(&f, task).await;
}

#[tokio::test]
#[ignore = "requires fresh disposable PostgreSQL and Redis; run coding_inline tests separately"]
async fn coding_inline_admission_and_cleanup_survive_primary_pool_saturation_postgres() {
    let mut f = Fixture::new().await;
    let executor = Executor::new().await;
    let scoped_user = Uuid::new_v4();
    executor.state.lock().unwrap().authority = Some(scoped_user);
    executor.state.lock().unwrap().stall = Some("prepare".into());
    // Deliberately tiny fixture pool: the two real requests occupy every slot.
    // Production retains the existing primary/admission pool defaults.
    let mut primary_options =
        ConnectOptions::new(std::env::var("HEART_TEST_DATABASE_URL").unwrap());
    primary_options.max_connections(2);
    Arc::get_mut(&mut f.state).unwrap().db = Database::connect(primary_options).await.unwrap();
    configure(&mut f, &executor, 3, 1, 1);
    let (task, subtask) = f.seed("coding_challenge").await;
    let endpoint = app(&f).await;
    let ordinary_user = Uuid::new_v4();
    let route = path(task, subtask);
    let ordinary = f.call(
        &endpoint,
        ordinary_user,
        false,
        Method::POST,
        &route,
        solution(),
    );
    let scoped = async {
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/learning{route}").parse().unwrap())
            .header(
                "x-learning-key",
                "synthetic-learning-key-with-at-least-43-characters",
            )
            .header("Content-Type", "application/json")
            .body(solution().to_string());
        endpoint
            .call(request)
            .await
            .unwrap()
            .into_response()
            .status()
            .as_u16()
    };
    let cleanup = async {
        executor.reached_count("prepare", 2).await;
        // Both request connections remain occupied, but committed admission
        // and exact-UUID cleanup can still use their independent short pool.
        let admission = Database::connect(f.config.database.url.to_string())
            .await
            .unwrap();
        let mut guard = InlineRun::reserve(
            &admission,
            Uuid::new_v4(),
            subtask,
            &f.config.challenges.coding_challenges.execution,
        )
        .await
        .unwrap()
        .unwrap();
        guard.release().await.unwrap();
    };
    let ((ordinary_status, body), scoped_status, ()) = tokio::join!(ordinary, scoped, cleanup);
    assert_eq!(ordinary_status, 503, "{body}");
    assert_eq!(scoped_status, 503);
    assert_eq!(
        executor
            .state
            .lock()
            .unwrap()
            .phases
            .iter()
            .filter(|p| p.as_str() == "prepare")
            .count(),
        2
    );
    cleaned(&f, ordinary_user).await;
    cleaned(&f, scoped_user).await;
    no_learning_effects(&f, ordinary_user).await;
    no_learning_effects(&f, scoped_user).await;
    remove_task(&f, task).await;
}

#[tokio::test]
#[ignore = "requires fresh disposable PostgreSQL and Redis; run coding_inline tests separately"]
async fn coding_inline_parallel_api_instances_are_bounded_postgres() {
    let mut f = Fixture::new().await;
    let executor = Executor::new().await;
    executor.state.lock().unwrap().stall = Some("prepare".into());
    configure(&mut f, &executor, 2, 1, 1);
    let (task, subtask) = f.seed("coding_challenge").await;
    let endpoints = [app(&f).await, app(&f).await];
    let user = Uuid::new_v4();
    let requests = (0..6).map(|i| {
        let endpoint = &endpoints[i % endpoints.len()];
        let f = &f;
        async move {
            f.call(
                endpoint,
                user,
                false,
                Method::POST,
                &path(task, subtask),
                solution(),
            )
            .await
            .0
        }
    });
    let results = futures::future::join_all(requests).await;
    assert_eq!(
        results.iter().filter(|status| **status == 429).count(),
        5,
        "{results:?}"
    );
    assert_eq!(
        results.iter().filter(|status| **status == 503).count(),
        1,
        "{results:?}"
    );
    assert_eq!(
        executor
            .state
            .lock()
            .unwrap()
            .phases
            .iter()
            .filter(|p| p.as_str() == "prepare")
            .count(),
        1
    );
    cleaned(&f, user).await;
    no_learning_effects(&f, user).await;
    assert_eq!(f.shop.lock().unwrap().balances[&user], 6);
    remove_task(&f, task).await;
}

#[tokio::test]
#[ignore = "requires fresh disposable PostgreSQL and Redis; run coding_inline tests separately"]
async fn coding_inline_sandbox_failures_are_free_and_retryable_postgres() {
    use crate::services::sandbox::tests::run_result;

    let mut f = Fixture::new().await;
    let executor = Executor::new().await;
    configure(&mut f, &executor, 1, 1, 5);
    let (task, subtask) = f.seed("coding_challenge").await;
    let user = Uuid::new_v4();
    let endpoint = app(&f).await;
    let mut failures: Vec<(StatusCode, String)> = [
        (
            StatusCode::BAD_REQUEST,
            run_result(1, "No space left on device"),
        ),
        (StatusCode::BAD_REQUEST, run_result(137, "")),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            run_result(1, "syntax error"),
        ),
    ]
    .into_iter()
    .map(|(status, details)| {
        (
            status,
            json!({"error":"compile_error","details":details}).to_string(),
        )
    })
    .collect();
    // Real full-cache replies (C/C++/Go/Rust link or write step) and nsjail
    // launch failures recorded from a local Sandkasten with production limits.
    let replies: Vec<Value> =
        serde_json::from_str(include_str!("../../services/sandbox/real_replies.json")).unwrap();
    for source in [
        "claude-fix:cache_full_cgroup:c_valid",
        "claude-fix:cache_full_cgroup:cpp_valid",
        "claude-fix:cache_full_cgroup:go_valid",
        "claude-fix:cache_full_cgroup:rust_valid",
        "claude-fix:launcher_failure:python_launcher_failure",
    ] {
        let reply = replies
            .iter()
            .find(|reply| reply["source"] == source)
            .unwrap();
        failures.push((
            StatusCode::from_u16(reply["http"].as_u64().unwrap() as u16).unwrap(),
            reply["body"].to_string(),
        ));
    }
    for (status, reply) in failures {
        executor.state.lock().unwrap().reply = Some(("solution".into(), status, reply));
        let mut data = solution();
        data["code"] = Uuid::new_v4().to_string().into();
        let (status, body) = f
            .call(
                &endpoint,
                user,
                false,
                Method::POST,
                &path(task, subtask),
                data,
            )
            .await;
        assert_eq!(status, 503, "{body}");
        assert_eq!(body["error"], "coding_execution_unavailable");
        assert!(body["detail"].as_str().unwrap().contains("keine Herzen"));
        cleaned(&f, user).await;
        no_learning_effects(&f, user).await;
        // Admission books the initial lesson once; technical retries book none.
        assert_eq!(f.shop.lock().unwrap().started.len(), 1);
    }
    // A genuine source error still reaches the learner, and the next test can run.
    executor.state.lock().unwrap().reply = Some((
        "solution".into(),
        StatusCode::BAD_REQUEST,
        json!({"error":"compile_error","details":run_result(1,"Main.java:1: error: ';' expected")})
            .to_string(),
    ));
    let mut data = solution();
    data["code"] = Uuid::new_v4().to_string().into();
    let (status, body) = f
        .call(
            &endpoint,
            user,
            false,
            Method::POST,
            &path(task, subtask),
            data,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["verdict"], "COMPILATION_ERROR");
    // A real Bash typo (status 127) is the learner's runtime error, with output.
    let typo = replies
        .iter()
        .find(|reply| reply["source"] == "claude-fix:cgroup_prod_config:bash_command_typo")
        .unwrap();
    executor.state.lock().unwrap().reply =
        Some(("solution".into(), StatusCode::OK, typo["body"].to_string()));
    let mut data = solution();
    data["code"] = Uuid::new_v4().to_string().into();
    let (status, body) = f
        .call(
            &endpoint,
            user,
            false,
            Method::POST,
            &path(task, subtask),
            data,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["verdict"], "RUNTIME_ERROR");
    assert_eq!(body["run"]["status"], 127);
    assert!(body["run"]["stderr"]
        .as_str()
        .unwrap()
        .contains("ech: command not found"));
    executor.state.lock().unwrap().reply = None;
    let mut data = solution();
    data["code"] = Uuid::new_v4().to_string().into();
    let (status, body) = f
        .call(
            &endpoint,
            user,
            false,
            Method::POST,
            &path(task, subtask),
            data,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["verdict"], "WRONG_ANSWER");
    cleaned(&f, user).await;
    no_learning_effects(&f, user).await;
    remove_task(&f, task).await;
}

#[tokio::test]
#[ignore = "requires fresh disposable PostgreSQL and Redis; run coding_inline tests separately"]
async fn coding_inline_deadline_covers_every_executor_phase_postgres() {
    for phase in ["examples", "generate", "prepare", "solution", "check"] {
        let mut f = Fixture::new().await;
        let executor = Executor::new().await;
        executor.state.lock().unwrap().stall = Some(phase.into());
        configure(&mut f, &executor, 1, 1, 1);
        let (task, subtask) = f.seed("coding_challenge").await;
        let user = Uuid::new_v4();
        let endpoint = app(&f).await;
        let began = std::time::Instant::now();
        let (status, body) = f
            .call(
                &endpoint,
                user,
                false,
                Method::POST,
                &path(task, subtask),
                solution(),
            )
            .await;
        assert_eq!(status, 503, "{phase}: {body}");
        assert_eq!(body["error"], "coding_execution_unavailable");
        assert_eq!(
            body["retry_after"],
            f.config
                .challenges
                .coding_challenges
                .execution
                .retry_seconds
        );
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "{phase}: {:?}",
            began.elapsed()
        );
        assert_eq!(executor.state.lock().unwrap().phases.last().unwrap(), phase);
        cleaned(&f, user).await;
        no_learning_effects(&f, user).await;
        // No later evaluator phase runs after the dropped HTTP future. A fresh
        // retry can use the freed slot and still creates no attempt or debit.
        executor.state.lock().unwrap().stall = None;
        if phase == "examples" {
            executor.state.lock().unwrap().fail = true;
            let (status, body) = f
                .call(
                    &endpoint,
                    user,
                    false,
                    Method::POST,
                    &path(task, subtask),
                    solution(),
                )
                .await;
            assert_eq!(status, 503, "transport failure: {body}");
            cleaned(&f, user).await;
            no_learning_effects(&f, user).await;
            executor.state.lock().unwrap().fail = false;
        }
        let (status, body) = f
            .call(
                &endpoint,
                user,
                false,
                Method::POST,
                &path(task, subtask),
                solution(),
            )
            .await;
        assert_eq!(status, 200, "{phase}: retry {body}");
        assert_eq!(body["verdict"], "WRONG_ANSWER");
        no_learning_effects(&f, user).await;
        remove_task(&f, task).await;
    }
}

#[tokio::test]
#[ignore = "requires fresh disposable PostgreSQL and Redis; run coding_inline tests separately"]
async fn coding_inline_cancellation_expiry_and_erasure_postgres() {
    let mut f = Fixture::new().await;
    let executor = Executor::new().await;
    executor.state.lock().unwrap().stall = Some("prepare".into());
    configure(&mut f, &executor, 1, 1, 10);
    let (task, subtask) = f.seed("coding_challenge").await;
    let user = Uuid::new_v4();
    let endpoint = app(&f).await;
    {
        let route = path(task, subtask);
        let request = f.call(&endpoint, user, false, Method::POST, &route, solution());
        tokio::pin!(request);
        tokio::select! {
            _ = &mut request => panic!("test must still be waiting on executor"),
            _ = executor.reached("prepare") => {},
        }
        assert_eq!(inline_count(&f, user).await, 1);
        // Drop the in-flight request, as on client cancellation.
    }
    cleaned(&f, user).await;
    no_learning_effects(&f, user).await;

    let stale = InlineRun::reserve(
        &f.state.db,
        user,
        subtask,
        &f.config.challenges.coding_challenges.execution,
    )
    .await
    .unwrap()
    .unwrap();
    f.state.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "UPDATE challenge_coding_inline_runs SET expires_at=clock_timestamp()-interval '1 second' WHERE user_id=$1",
        [user.into()])).await.unwrap();
    let replacement = InlineRun::reserve(
        &f.state.db,
        user,
        subtask,
        &f.config.challenges.coding_challenges.execution,
    )
    .await
    .unwrap()
    .unwrap();
    drop(stale);
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(
        inline_count(&f, user).await,
        1,
        "old cleanup cannot remove replacement UUID"
    );

    let erasure = f.state.db.begin().await.unwrap();
    delete_user_data(&erasure, user).await.unwrap();
    erasure.commit().await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), replacement.until_removed(10))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(inline_count(&f, user).await, 0);
    assert!(InlineRun::reserve(
        &f.state.db,
        user,
        subtask,
        &f.config.challenges.coding_challenges.execution
    )
    .await
    .is_err());
    drop(replacement);
    no_learning_effects(&f, user).await;

    // Committed erasure also interrupts the actual request's executor future.
    let erased = Uuid::new_v4();
    executor.state.lock().unwrap().phases.clear();
    let route = path(task, subtask);
    let request = f.call(&endpoint, erased, false, Method::POST, &route, solution());
    let erase = async {
        executor.reached("prepare").await;
        let transaction = f.state.db.begin().await.unwrap();
        delete_user_data(&transaction, erased).await.unwrap();
        transaction.commit().await.unwrap();
    };
    let began = std::time::Instant::now();
    let ((status, body), ()) = tokio::join!(request, erase);
    assert_eq!(status, 503, "{body}");
    assert!(began.elapsed() < Duration::from_secs(2));
    cleaned(&f, erased).await;
    no_learning_effects(&f, erased).await;
    remove_task(&f, task).await;
}

#[tokio::test]
#[ignore = "requires fresh disposable PostgreSQL and Redis; run coding_inline tests separately"]
async fn coding_inline_scoped_route_shares_admission_without_subject_deadlock_postgres() {
    let mut f = Fixture::new().await;
    let executor = Executor::new().await;
    let user = Uuid::new_v4();
    executor.state.lock().unwrap().authority = Some(user);
    configure(&mut f, &executor, 1, 1, 1);
    let (task, subtask) = f.seed("coding_challenge").await;
    let endpoint = app(&f).await;
    let call = || async {
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/learning{}", path(task, subtask)).parse().unwrap())
            .header(
                "x-learning-key",
                "synthetic-learning-key-with-at-least-43-characters",
            )
            .header("Content-Type", "application/json")
            .body(solution().to_string());
        endpoint.call(request).await.unwrap().into_response()
    };
    let response = call().await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "scoped refresh must release its subject lock"
    );
    no_learning_effects(&f, user).await;
    let mut lease = InlineRun::reserve(
        &f.state.db,
        user,
        subtask,
        &f.config.challenges.coding_challenges.execution,
    )
    .await
    .unwrap()
    .unwrap();
    let response = call().await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers()["Retry-After"], "10");
    lease.release().await.unwrap();
    executor.state.lock().unwrap().stall = Some("prepare".into());
    let response = call().await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["Retry-After"], "10");
    cleaned(&f, user).await;
    no_learning_effects(&f, user).await;
    remove_task(&f, task).await;
}
