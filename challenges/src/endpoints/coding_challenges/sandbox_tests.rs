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
    stall: bool,
    calls: usize,
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
                        (state.fault.clone(), state.stall)
                    };
                    if stall {
                        tokio::time::sleep(Duration::from_secs(3)).await;
                    }
                    if let Some((at, status, body)) = fault {
                        if at == phase {
                            return Response::builder().status(status).body(body);
                        }
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
        json!({"error":"compile_error","details":run_result(1,"Main.java:1: error: ';' expected")})
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
    // Genuine compilation errors still persist exactly one learner attempt and heart operation.
    let user = Uuid::new_v4();
    let (_, submission) = seed(&f, user).await;
    executor.state.lock().unwrap().fault =
        Some(("solution".into(), StatusCode::BAD_REQUEST, syntax));
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
