//! Real judge/queue/result transactions with loopback executor responses.
use std::sync::Mutex;

use poem::{
    endpoint::make,
    http::StatusCode,
    listener::{Acceptor, Listener, TcpListener},
    Request, Response, Server,
};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::{json, Value};

use super::*;
use crate::{
    endpoints::heart_tests::Fixture,
    services::sandbox::tests::{run_result, success},
};

#[derive(Default)]
struct ExecutorState {
    fault: Option<(String, StatusCode, String)>,
    /// Serve the fault this many times, then answer normally again.
    fault_limit: Option<usize>,
    stall: bool,
    calls: usize,
    solution_calls: usize,
}

struct Executor {
    client: SandkastenClient,
    state: Arc<Mutex<ExecutorState>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Executor {
    fn drop(&mut self) {
        self.task.abort();
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
                    let body: Value = request.take_body().into_json().await.unwrap();
                    let phase = body["run"]["args"][0].as_str().unwrap_or("solution");
                    let (fault, stall) = {
                        let mut state = state.lock().unwrap();
                        state.calls += 1;
                        if phase == "solution" {
                            state.solution_calls += 1;
                        }
                        let fault = state.fault.clone().filter(|(at, ..)| at == phase);
                        let fault = match (fault, state.fault_limit) {
                            (Some(_), Some(0)) => None,
                            (Some(fault), limit) => {
                                state.fault_limit = limit.map(|n| n - 1);
                                Some(fault)
                            }
                            (None, _) => None,
                        };
                        (fault, state.stall)
                    };
                    if stall {
                        tokio::time::sleep(Duration::from_secs(3)).await;
                    }
                    if let Some((_, status, body)) = fault {
                        return Response::builder().status(status).body(body);
                    }
                    let stdout = match phase {
                        "examples" => json!(["example"]).to_string(),
                        "generate" => json!({"input":"42","data":null}).to_string(),
                        "prepare" => json!({"code":"synthetic","reason":""}).to_string(),
                        "solution" => "42".into(),
                        "check" => json!({"verdict":"OK","reason":null}).to_string(),
                        _ => panic!("unknown evaluator phase"),
                    };
                    Response::builder()
                        .content_type("application/json")
                        .body(success(&stdout).to_string())
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
        let task = tokio::spawn(async move {
            Server::new_with_acceptor(acceptor)
                .run(endpoint)
                .await
                .unwrap();
        });
        Self {
            client: SandkastenClient::new(format!("http://{address}/").parse().unwrap()),
            state,
            task,
        }
    }
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

async fn no_effects(f: &Fixture, submission: Uuid, user: Uuid, starts: usize) {
    for table in [
        "challenges_user_subtasks",
        "challenge_heart_operations",
        "challenge_benefit_earnings",
    ] {
        assert_eq!(count(f, table, "user_id", user).await, 0, "{table}");
    }
    assert!(
        challenges_coding_challenge_result::Entity::find_by_id(submission)
            .one(&f.state.db)
            .await
            .unwrap()
            .is_none()
    );
    let shop = f.shop.lock().unwrap();
    assert_eq!(shop.calls, 0, "no heart settlement");
    assert!(shop
        .xp_operations
        .values()
        .all(|operation| operation["user_id"] != user.to_string()));
    assert_eq!(shop.started.len(), starts, "no additional daily start");
}

/// A recorded real Sandkasten reply (see `services/sandbox/tests.rs`).
fn real_reply(source: &str) -> (StatusCode, String) {
    let replies: Vec<Value> =
        serde_json::from_str(include_str!("../../services/sandbox/real_replies.json")).unwrap();
    let reply = replies
        .into_iter()
        .find(|reply| reply["source"] == source)
        .expect("recorded reply");
    (
        StatusCode::from_u16(reply["http"].as_u64().unwrap() as u16).unwrap(),
        reply["body"].to_string(),
    )
}

async fn seed(
    f: &Fixture,
    user: Uuid,
) -> (
    challenges_subtasks::Model,
    challenges_coding_challenge_submissions::Model,
) {
    let (_, subtask) = f.seed("coding_challenge").await;
    // Every case has distinct evaluator cache keys, including faulted examples.
    f.state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "UPDATE challenges_coding_challenges SET evaluator=$2 WHERE subtask_id=$1",
            [
                subtask.into(),
                format!("synthetic evaluator {subtask}").into(),
            ],
        ))
        .await
        .unwrap();
    let subtask = challenges_subtasks::Entity::find_by_id(subtask)
        .one(&f.state.db)
        .await
        .unwrap()
        .unwrap();
    let submission = challenges_coding_challenge_submissions::ActiveModel {
        id: Set(Uuid::new_v4()),
        subtask_id: Set(subtask.id),
        creator: Set(user),
        creation_timestamp: Set(Utc::now().naive_utc()),
        environment: Set("java".into()),
        code: Set(format!("synthetic code {user}")),
        charge_on_failure: Set(true),
        ..Default::default()
    }
    .insert(&f.state.db)
    .await
    .unwrap();
    (subtask, submission)
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn coding_sandbox_failure_paths_and_recovery_postgres() {
    let f = Fixture::new().await;
    let executor = Executor::new().await;
    let syntax =
        json!({"error":"compile_error","details":run_result(1,"Main.java:1: error: ';' expected\nSystem.out.println(\"No space left on device\")")})
            .to_string();
    let mut resource = run_result(1, "");
    resource["resource_usage"]["memory"] = (128 * 1024).into();
    let mut faults = vec![
        ("solution", StatusCode::BAD_REQUEST, json!({"error":"compile_error","details":run_result(1,"java.io.IOException: No space left on device")}).to_string()),
        ("solution", StatusCode::BAD_REQUEST, json!({"error":"compile_error","details":run_result(137,"")}).to_string()),
        ("solution", StatusCode::BAD_REQUEST, json!({"error":"compile_error","details":resource}).to_string()),
        ("solution", StatusCode::INTERNAL_SERVER_ERROR, syntax.clone()),
        ("solution", StatusCode::OK, "{}".into()),
        ("solution", StatusCode::OK, "{\"run\":".into()),
        ("solution", StatusCode::BAD_REQUEST, "{\"error\":\"compile_error\"}".into()),
    ];
    // Real full-cache and launcher replies; status 255 alone would be the learner's.
    for source in [
        "review:cache_full_cgroup:c",
        "claude-fix:cache_full_cgroup:cpp_valid",
        "claude-fix:cache_full_cgroup:go_valid",
        "claude-fix:cache_full_cgroup:rust_valid",
        "claude-fix:cache_full_cgroup:csharp_valid",
        "claude-fix:launcher_failure:python_launcher_failure",
        "claude-fix:launcher_failure:c_launcher_failure",
    ] {
        let (status, body) = real_reply(source);
        faults.push(("solution", status, body));
    }
    for phase in ["examples", "generate", "prepare", "check"] {
        faults.push((phase, StatusCode::SERVICE_UNAVAILABLE, "unavailable".into()));
        faults.push((
            phase,
            StatusCode::OK,
            success("not evaluator JSON").to_string(),
        ));
        let mut failed = success("invalid evaluator output");
        failed["run"]["status"] = 1.into();
        faults.push((phase, StatusCode::OK, failed.to_string()));
    }
    for (phase, status, body) in faults {
        let user = Uuid::new_v4();
        let (_, submission) = seed(&f, user).await;
        executor.state.lock().unwrap().fault = Some((phase.into(), status, body));
        let starts = f.shop.lock().unwrap().started.len();
        let claim = queue::claim(&f.state.db, Uuid::new_v4(), 30)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claim.submission, submission.id);
        assert!(
            execute_claim(f.state.clone(), &executor.client, claim)
                .await
                .is_err(),
            "{phase}/{status}"
        );
        no_effects(&f, submission.id, user, starts).await;
        queue::retry(&f.state.db, claim, 1).await.unwrap();
        let pending = challenges_coding_challenge_submissions::Entity::find_by_id(submission.id)
            .one(&f.state.db)
            .await
            .unwrap()
            .unwrap();
        assert!(pending.judge_pending);
        assert!(pending.judge_lease_owner.is_none());
        executor.state.lock().unwrap().fault = None;
        f.state.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
            "UPDATE challenges_coding_challenge_submissions SET judge_available_at=now()-interval '1 second' WHERE id=$1", [submission.id.into()])).await.unwrap();
        let retry = queue::claim(&f.state.db, claim.owner, 30)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retry.submission, claim.submission);
        assert!(retry.generation > claim.generation);
        execute_claim(f.state.clone(), &executor.client, retry)
            .await
            .unwrap();
        assert_eq!(
            count(&f, "challenge_heart_operations", "user_id", user).await,
            0
        );
        assert_eq!(
            count(&f, "challenge_benefit_earnings", "user_id", user).await,
            1
        );
        assert_eq!(f.shop.lock().unwrap().started.len(), starts);
        let progress = challenges_user_subtasks::Entity::find_by_id((user, pending.subtask_id))
            .one(&f.state.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(progress.attempts, 1, "only the completed retry counts");
        assert_eq!(
            challenges_coding_challenge_result::Entity::find_by_id(submission.id)
                .one(&f.state.db)
                .await
                .unwrap()
                .unwrap()
                .verdict,
            ChallengesVerdict::Ok
        );
    }
    // Genuine compilation errors still persist exactly one learner attempt and
    // heart operation, also when learner text names an infrastructure failure.
    let mut compile_errors = vec![(StatusCode::BAD_REQUEST, syntax)];
    for source in [
        "claude-fix:cgroup_prod_config:c_error_directive_enospc",
        "claude-fix:cgroup_prod_config:cpp_static_assert_out_of_memory",
        "claude-fix:cgroup_prod_config:go_import_enospc",
        "claude-fix:cgroup_prod_config:rust_compile_error_macro_enospc",
        "claude-fix:cgroup_prod_config:java_string_echo_enospc",
        "claude-fix:cgroup_prod_config:kotlin_compile_error",
    ] {
        compile_errors.push(real_reply(source));
    }
    for (status, body) in compile_errors {
        let user = Uuid::new_v4();
        let (_, submission) = seed(&f, user).await;
        executor.state.lock().unwrap().fault = Some(("solution".into(), status, body));
        let claim = queue::claim(&f.state.db, Uuid::new_v4(), 30)
            .await
            .unwrap()
            .unwrap();
        execute_claim(f.state.clone(), &executor.client, claim)
            .await
            .unwrap();
        assert_eq!(
            challenges_coding_challenge_result::Entity::find_by_id(submission.id)
                .one(&f.state.db)
                .await
                .unwrap()
                .unwrap()
                .verdict,
            ChallengesVerdict::CompilationError
        );
        assert_eq!(
            count(&f, "challenge_heart_operations", "user_id", user).await,
            1
        );
        assert_eq!(
            count(&f, "challenge_benefit_earnings", "user_id", user).await,
            0
        );
    }
    // Runtime failures and lesson resource violations remain learner errors.
    // An exit status alone is never technical: 127 is a Bash typo, 137 may be
    // `exit 137` or the cgroup OOM kill at the lesson limit (RSS 125276 KiB of
    // 128 MB, measured below 128 MiB).
    for (status, time, memory, verdict) in [
        (1, 10, 1024, ChallengesVerdict::RuntimeError),
        (127, 10, 1024, ChallengesVerdict::RuntimeError),
        (137, 10, 1024, ChallengesVerdict::RuntimeError),
        (255, 10, 1024, ChallengesVerdict::RuntimeError),
        (137, 1001, 1024, ChallengesVerdict::TimeLimitExceeded),
        (137, 10, 129 * 1024, ChallengesVerdict::MemoryLimitExceeded),
        (137, 10, 125276, ChallengesVerdict::MemoryLimitExceeded),
    ] {
        let user = Uuid::new_v4();
        let (_, submission) = seed(&f, user).await;
        let mut output = success("42");
        output["run"]["status"] = status.into();
        output["run"]["resource_usage"]["time"] = time.into();
        output["run"]["resource_usage"]["memory"] = memory.into();
        output["run"]["stderr"] = "No space left on device".into();
        executor.state.lock().unwrap().fault =
            Some(("solution".into(), StatusCode::OK, output.to_string()));
        let claim = queue::claim(&f.state.db, Uuid::new_v4(), 30)
            .await
            .unwrap()
            .unwrap();
        execute_claim(f.state.clone(), &executor.client, claim)
            .await
            .unwrap();
        assert_eq!(
            challenges_coding_challenge_result::Entity::find_by_id(submission.id)
                .one(&f.state.db)
                .await
                .unwrap()
                .unwrap()
                .verdict,
            verdict
        );
        assert_eq!(
            count(&f, "challenge_heart_operations", "user_id", user).await,
            1
        );
        assert_eq!(
            count(&f, "challenge_benefit_earnings", "user_id", user).await,
            0
        );
    }
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn coding_sandbox_legacy_result_guard_and_worker_deadline_postgres() {
    let mut f = Fixture::new().await;
    let executor = Executor::new().await;
    let user = Uuid::new_v4();
    let (subtask, submission) = seed(&f, user).await;
    let claim = queue::claim(&f.state.db, Uuid::new_v4(), 30)
        .await
        .unwrap()
        .unwrap();
    let result = CheckError::TestcaseFailed(CheckTestcaseError {
        seed: "legacy cache".into(),
        result: schemas::challenges::coding_challenges::CheckResult {
            verdict: ChallengesVerdict::CompilationError,
            reason: None,
            run: None,
            compile: Some(serde_json::from_value(run_result(1, "ENOSPC")).unwrap()),
        },
    });
    assert!(
        record_claimed_judgment(f.state.clone(), claim, &subtask, &submission, Err(result))
            .await
            .is_err()
    );
    no_effects(&f, submission.id, user, 0).await;
    executor.state.lock().unwrap().stall = true;
    assert!(tokio::time::timeout(
        Duration::from_millis(50),
        execute_claim(f.state.clone(), &executor.client, claim)
    )
    .await
    .is_err());
    no_effects(&f, submission.id, user, 0).await;
    // Actual worker deadline and automatic postponement, rather than a fabricated grade.
    queue::retry(&f.state.db, claim, 1).await.unwrap();
    f.state.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
        "UPDATE challenges_coding_challenge_submissions SET judge_available_at=now()-interval '1 second' WHERE id=$1", [submission.id.into()])).await.unwrap();
    let config = Arc::get_mut(&mut f.config).unwrap();
    config.challenges.coding_challenges.max_concurrency = 1;
    config
        .challenges
        .coding_challenges
        .execution
        .max_execution_seconds = 1;
    config.challenges.coding_challenges.execution.retry_seconds = 30;
    config
        .challenges
        .coding_challenges
        .execution
        .poll_milliseconds = 10;
    let worker = tokio::spawn(run_worker(
        f.state.clone(),
        f.config.clone(),
        executor.client.clone(),
    ));
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let row = challenges_coding_challenge_submissions::Entity::find_by_id(submission.id)
                .one(&f.state.db)
                .await
                .unwrap()
                .unwrap();
            if row.judge_generation > claim.generation && row.judge_lease_owner.is_none() {
                assert!(row.judge_pending);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("worker automatically postponed technical timeout");
    worker.abort();
    let _ = worker.await;
    no_effects(&f, submission.id, user, 0).await;
    // Leave no pending fixture that could interfere with another queue test.
    f.state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "DELETE FROM challenges_coding_challenge_submissions WHERE id=$1",
            [submission.id.into()],
        ))
        .await
        .unwrap();
}

async fn settled(f: &Fixture, submission: Uuid) -> challenges_coding_challenge_submissions::Model {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let row = challenges_coding_challenge_submissions::Entity::find_by_id(submission)
                .one(&f.state.db)
                .await
                .unwrap()
                .unwrap();
            if !row.judge_pending {
                return row;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("submission settled")
}

async fn listed(f: &Fixture, api: &Api, subtask: &challenges_subtasks::Model, user: Uuid) -> Value {
    use poem::IntoResponse;
    let tx = Arc::new(f.state.db.begin().await.unwrap());
    api.list_submission_result(
        Path(subtask.task_id),
        Path(subtask.id),
        Data(&tx),
        VerifiedUserAuth(lib::auth::User {
            id: user,
            email_verified: true,
            admin: false,
        }),
        false,
    )
    .await
    .unwrap()
    .into_response()
    .into_body()
    .into_json()
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn coding_sandbox_technical_attempt_cap_postgres() {
    let mut f = Fixture::new().await;
    let executor = Executor::new().await;
    let config = Arc::get_mut(&mut f.config).unwrap();
    config.challenges.coding_challenges.max_concurrency = 1;
    let settings = &mut config.challenges.coding_challenges.execution;
    settings.max_technical_attempts = 3;
    settings.retry_seconds = 1;
    settings.poll_milliseconds = 10;
    settings.max_pending_per_user = 1;
    let settings = settings.clone();
    let api = Api {
        state: f.state.clone(),
        config: f.config.clone(),
        sandkasten: executor.client.clone(),
        judge_cache: f.state.cache.with_formatter(JsonFormatter),
    };
    let worker = tokio::spawn(run_worker(
        f.state.clone(),
        f.config.clone(),
        executor.client.clone(),
    ));
    let solution_calls = || executor.state.lock().unwrap().solution_calls;

    // A persistent real outage (full artifact cache, C link step) gets exactly
    // three free attempts with backoff, then closes without verdict or cost.
    let user = Uuid::new_v4();
    let (subtask, submission) = seed(&f, user).await;
    let (status, body) = real_reply("claude-fix:cache_full_cgroup:c_valid");
    executor.state.lock().unwrap().fault = Some(("solution".into(), status, body));
    let started = std::time::Instant::now();
    let row = settled(&f, submission.id).await;
    assert!(
        started.elapsed() >= Duration::from_secs(3),
        "backoff 1 s + 2 s"
    );
    assert_eq!(solution_calls(), 3);
    assert_eq!(row.judge_generation, 3);
    assert!(row.judge_lease_owner.is_none());
    no_effects(&f, submission.id, user, 0).await;
    // Honest final state for the learner, and the pending slot is free again.
    let list = listed(&f, &api, &subtask, user).await;
    assert_eq!(list[0]["id"], json!(submission.id));
    assert_eq!(list[0]["technical_failure"], true, "{list}");
    assert!(list[0]["result"].is_null() && list[0]["queue_position"].is_null());
    let tx = f.state.db.begin().await.unwrap();
    assert!(queue::admit(&tx, user, &settings).await.unwrap());
    tx.rollback().await.unwrap();

    // Earlier claims lost their lease (e.g. the worker crashed): no fourth run.
    let user = Uuid::new_v4();
    let (_, submission) = seed(&f, user).await;
    f.state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "UPDATE challenges_coding_challenge_submissions SET judge_generation=3 WHERE id=$1",
            [submission.id.into()],
        ))
        .await
        .unwrap();
    let row = settled(&f, submission.id).await;
    assert_eq!(row.judge_generation, 4);
    assert_eq!(solution_calls(), 3, "closed without executing again");
    no_effects(&f, submission.id, user, 0).await;

    // A transient outage that recovers within the cap is judged normally once.
    let user = Uuid::new_v4();
    let (_, submission) = seed(&f, user).await;
    {
        let mut state = executor.state.lock().unwrap();
        state.fault = Some((
            "solution".into(),
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable".into(),
        ));
        state.fault_limit = Some(2);
    }
    let row = settled(&f, submission.id).await;
    // Two failed attempts, then the third judges every test case normally.
    assert_eq!(row.judge_generation, 3);
    assert_eq!(executor.state.lock().unwrap().fault_limit, Some(0));
    let result = challenges_coding_challenge_result::Entity::find_by_id(submission.id)
        .one(&f.state.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.verdict, ChallengesVerdict::Ok);
    assert_eq!(
        count(&f, "challenge_heart_operations", "user_id", user).await,
        0
    );
    assert_eq!(
        count(&f, "challenge_benefit_earnings", "user_id", user).await,
        1
    );

    // Real learner errors are judged on the first attempt and are never retried.
    for (source, verdict) in [
        (
            "claude-fix:cgroup_prod_config:bash_command_typo",
            ChallengesVerdict::RuntimeError,
        ),
        (
            "claude-fix:cgroup_prod_config:c_memory_lesson_64mb",
            ChallengesVerdict::MemoryLimitExceeded,
        ),
    ] {
        let user = Uuid::new_v4();
        let (_, submission) = seed(&f, user).await;
        let before = solution_calls();
        {
            let mut state = executor.state.lock().unwrap();
            let (status, body) = real_reply(source);
            state.fault = Some(("solution".into(), status, body));
            state.fault_limit = None;
        }
        let row = settled(&f, submission.id).await;
        assert_eq!(row.judge_generation, 1, "{source}");
        assert_eq!(solution_calls(), before + 1, "{source}");
        let result = challenges_coding_challenge_result::Entity::find_by_id(submission.id)
            .one(&f.state.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.verdict, verdict, "{source}");
        assert_eq!(
            count(&f, "challenge_heart_operations", "user_id", user).await,
            1,
            "{source}"
        );
        assert_eq!(
            count(&f, "challenge_benefit_earnings", "user_id", user).await,
            0,
            "{source}"
        );
    }
    worker.abort();
    let _ = worker.await;
}
