//! Real routes, authenticated synthetic subjects, isolated PostgreSQL/Redis and
//! a local Shop stub. No live account, executor, mailbox or service is contacted.
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use fnct::{backend::AsyncRedisBackend, format::PostcardFormatter};
use lib::{
    config::Config,
    jwt::{
        sign_jwt, verify_jwt, InternalJwtSecrets, JwtSecret, UserAccessToken, UserAccessTokenData,
    },
    redis::RedisConnection,
    services::Services,
    Cache, SharedState,
};
use poem::{
    endpoint::make,
    listener::{Acceptor, Listener, TcpListener},
    Endpoint, EndpointExt, IntoResponse, Request, Response, Route, Server,
};
use poem_ext::db::DbTransactionMiddleware;
use poem_openapi::OpenApiService;
use sea_orm::{ConnectionTrait, Database, DbBackend, Statement, TransactionTrait};
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct Shop {
    pub balances: HashMap<Uuid, u64>,
    pub premium: HashSet<Uuid>,
    pub receipts: HashMap<Uuid, Value>,
    pub lose_reply: bool,
    pub calls: usize,
    /// skills-ms XP operations received, by operation ID.
    pub xp_operations: HashMap<Uuid, Value>,
    pub modes: HashMap<Uuid, String>,
    pub policy_status: Option<u16>,
    pub policy_body: Option<Value>,
    pub heart_reads: usize,
    pub learner_executions: usize,
    pub deny_courses: HashSet<String>,
    pub deny_subtasks: HashSet<Uuid>,
    pub deny_new_starts: HashSet<Uuid>,
    pub access_status: Option<u16>,
    pub access_requests: Vec<(Uuid, String, Value)>,
    pub read_batch_override: Option<Value>,
    pub started: HashSet<(Uuid, Uuid)>,
    pub heart_policies: HashMap<(Uuid, Uuid), String>,
    pub heart_status: Option<u16>,
    pub malformed_receipt: bool,
    /// Confirmed monthly renewal whose paid time ended. Backend's policy read
    /// answers 500 for it; its Premium read settles it (renews or, if the
    /// coins no longer cover it, ends the agreement).
    pub renewal_due: HashSet<Uuid>,
    pub renewal_unfunded: HashSet<Uuid>,
    pub renewals: usize,
    pub premium_reads: usize,
    pub premium_status: Option<u16>,
}

impl Shop {
    fn can_read(&self, body: &Value) -> bool {
        !body["lecture_bindings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|binding| {
                self.deny_courses
                    .contains(binding["course_id"].as_str().unwrap())
            })
            && !body["subtask_id"]
                .as_str()
                .is_some_and(|id| self.deny_subtasks.contains(&Uuid::parse_str(id).unwrap()))
    }
}

pub(crate) struct Fixture {
    pub state: Arc<SharedState>,
    pub config: Arc<Config>,
    pub shop: Arc<Mutex<Shop>>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Fixture {
    pub(crate) async fn new() -> Self {
        Self::with_access_reads(true).await
    }

    pub(crate) async fn with_access_reads(enabled: bool) -> Self {
        let db = Database::connect(
            std::env::var("HEART_TEST_DATABASE_URL").expect("isolated migrated PostgreSQL"),
        )
        .await
        .unwrap();
        let redis =
            RedisConnection::new(&std::env::var("HEART_TEST_REDIS_URL").expect("isolated Redis"))
                .await
                .unwrap();
        let shop = Arc::new(Mutex::new(Shop::default()));
        let secret = JwtSecret::try_from("synthetic-local-heart-tests").unwrap();
        let app = make({
            let shop = shop.clone();
            let secret = secret.clone();
            let db = db.clone();
            move |mut request: Request| {
                let shop = shop.clone();
                let secret = secret.clone();
                let db = db.clone();
                async move {
                    let path = request.uri().path().to_owned();
                    let value = if path == "/environments" {
                        json!({"python":{"name":"Python","version":"synthetic","default_main_file_name":"code.py","example":null,"meta":{}}})
                    } else if path == "/run" {
                        let body: Value = request.take_body().into_json().await.unwrap();
                        let stdout = match body["run"]["args"][0].as_str() {
                            Some("examples") => json!(["example"]),
                            Some("generate") => json!({"input":"synthetic", "data":null}),
                            Some("prepare") => {
                                shop.lock().unwrap().learner_executions += 1;
                                json!({"code":null,"reason":"synthetic precheck"})
                            }
                            _ => panic!("unexpected executor operation"),
                        };
                        json!({"program_id":Uuid::new_v4(),"ttl":60,"cached":false,"build":null,
                            "run":{"status":0,"stdout":stdout.to_string(),"stderr":"",
                                "resource_usage":{"time":1,"memory":1},
                                "limits":{"cpus":1,"time":1,"memory":128,"tmpfs":0,"filesize":1,
                                    "file_descriptors":32,"processes":1,"stdout_max_size":4096,
                                    "stderr_max_size":4096,"network":false}}})
                    } else if path.ends_with("/ordinary-authority") {
                        let body: Value = request.take_body().into_json().await.unwrap();
                        let user: UserAccessToken =
                            verify_jwt(body["access_token"].as_str().unwrap(), &secret).unwrap();
                        json!({"id":user.uid,"admin":user.data.admin,"email_verified":true})
                    } else if path.contains("/heart-operations/") {
                        let parts: Vec<_> = path.rsplit('/').collect();
                        let user: Uuid = parts[0].parse().unwrap();
                        let operation: Uuid = parts[1].parse().unwrap();
                        let body: Value = request.take_body().into_json().await.unwrap();
                        assert_eq!(
                            body,
                            json!({"half_hearts":2,"reason":"incorrect_challenge_attempt"})
                        );
                        // This is a different connection: producer commit must be visible.
                        assert!(db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
                            "SELECT 1 FROM challenge_heart_operations WHERE id=$1 AND user_id=$2", [operation.into(),user.into()])).await.unwrap().is_some());
                        let mut shop = shop.lock().unwrap();
                        shop.calls += 1;
                        if let Some(status) = shop.heart_status {
                            return Response::builder()
                                .status(poem::http::StatusCode::from_u16(status).unwrap())
                                .body("unavailable before commit");
                        }
                        let receipt = if let Some(receipt) = shop.receipts.get(&operation) {
                            receipt.clone()
                        } else {
                            let balance = *shop.balances.entry(user).or_insert(6);
                            let outcome =
                                if shop.modes.get(&user).is_some_and(|mode| mode == "daily") {
                                    "daily_learning"
                                } else if shop.premium.contains(&user) {
                                    "premium"
                                } else if balance < 2 {
                                    "insufficient"
                                } else {
                                    "charged"
                                };
                            let charged = if outcome == "charged" { 2 } else { 0 };
                            shop.balances.insert(user, balance - charged);
                            let receipt = json!({"operation_id":operation,"user_id":user,"charged_half_hearts":charged,"hearts":balance-charged,"outcome":outcome});
                            shop.receipts.insert(operation, receipt.clone());
                            receipt
                        };
                        if std::mem::take(&mut shop.lose_reply) {
                            return Response::builder()
                                .status(poem::http::StatusCode::SERVICE_UNAVAILABLE)
                                .body("lost after commit");
                        }
                        if shop.malformed_receipt {
                            let mut bad = receipt.clone();
                            bad["charged_half_hearts"] = json!(2);
                            bad["outcome"] = json!("daily_learning");
                            bad
                        } else {
                            receipt
                        }
                    } else if path.contains("/learning-policy/") {
                        let user: Uuid = path.rsplit('/').next().unwrap().parse().unwrap();
                        let shop = shop.lock().unwrap();
                        if let Some(status) = shop.policy_status {
                            return Response::builder()
                                .status(poem::http::StatusCode::from_u16(status).unwrap())
                                .body("policy unavailable");
                        }
                        if shop.renewal_due.contains(&user) {
                            return Response::builder()
                                .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
                                .body("Confirmed premium renewal is awaiting settlement");
                        }
                        shop.policy_body.clone().unwrap_or_else(|| json!({"mode":shop.modes.get(&user).map(String::as_str).unwrap_or("legacy"),"premium":shop.premium.contains(&user),"single_course_sales":true,"heart_sales":true}))
                    } else if path.contains("/learning-access/") {
                        let parts: Vec<_> = path.rsplit('/').collect();
                        let action = parts[0];
                        let token = request
                            .headers()
                            .get("authorization")
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .strip_prefix("Bearer ")
                            .unwrap();
                        let claims: lib::jwt::InternalAuthToken =
                            verify_jwt(token, &secret).unwrap();
                        assert_eq!(claims.aud, "skills");
                        let user: Uuid = parts[1].parse().unwrap();
                        let body: Value = request.take_body().into_json().await.unwrap();
                        let mut shop = shop.lock().unwrap();
                        shop.access_requests
                            .push((user, action.to_owned(), body.clone()));
                        if let Some(status) = shop.access_status {
                            return Response::builder()
                                .status(poem::http::StatusCode::from_u16(status).unwrap())
                                .body("access unavailable");
                        }
                        if action == "check-batch" {
                            let requests = body["requests"].as_array().unwrap();
                            assert!(!requests.is_empty() && requests.len() <= 250);
                            assert!(requests
                                .iter()
                                .all(|request| request.get("request_id").is_none()));
                            let value = shop.read_batch_override.clone().unwrap_or_else(|| {
                                json!({"readable": requests.iter().map(|request| shop.can_read(request)).collect::<Vec<_>>()})
                            });
                            return Response::builder()
                                .content_type("application/json")
                                .body(value.to_string());
                        }
                        let subtask = body["subtask_id"]
                            .as_str()
                            .map(|id| Uuid::parse_str(id).unwrap());
                        if !shop.can_read(&body) {
                            return Response::builder().status(poem::http::StatusCode::FORBIDDEN).content_type("application/json").body(json!({"code":"course_access_required","detail":"Course access required"}).to_string());
                        }
                        if action == "start" {
                            assert!(body["request_id"]
                                .as_str()
                                .is_some_and(|id| Uuid::parse_str(id).is_ok()));
                            let subtask = subtask.expect("start needs concrete subtask");
                            if shop.deny_new_starts.contains(&user)
                                && !shop.started.contains(&(user, subtask))
                            {
                                return Response::builder().status(poem::http::StatusCode::TOO_MANY_REQUESTS).content_type("application/json").body(json!({"code":"daily_limit_reached","detail":"You can keep practising.","daily":{"mode":"daily","limit":3,"used":3,"remaining":0,"unlimited":false,"resets_at":"2026-09-27T22:00:00Z","timezone":"Europe/Berlin"}}).to_string());
                            }
                            shop.started.insert((user, subtask));
                        } else {
                            assert_eq!(action, "check");
                            assert!(body.get("request_id").is_none());
                        }
                        json!({"allowed":true,"lesson":null,"daily":null,"heart_policy":subtask.and_then(|id|shop.heart_policies.get(&(user,id)))})
                    } else if path.ends_with("/_internal/skills") {
                        json!([
                            {"id":"synthetic-skill","parent_id":"root","courses":["synthetic-course","locked-course"]},
                            {"id":"synthetic-sub-skill","parent_id":"synthetic-root","courses":[]},
                            {"id":"other-sub-skill","parent_id":"synthetic-root","courses":[]}
                        ])
                    } else if path.starts_with("/skills/_internal/xp-operations/") {
                        let parts: Vec<_> = path.rsplit('/').collect();
                        let (skill, user, operation) = (parts[0], parts[1], parts[2]);
                        let operation: Uuid = operation.parse().unwrap();
                        let body: Value = request.take_body().into_json().await.unwrap();
                        let exact = json!({"user_id":user,"skill_id":skill,"xp":body["xp"],"earning_id":body["earning_id"]});
                        let mut shop = shop.lock().unwrap();
                        let known = shop.xp_operations.entry(operation).or_insert(exact.clone());
                        assert_eq!(known, &exact);
                        json!({"operation_id":operation,"request":exact,"state":"applied","applied":true})
                    } else if path.contains("/premium/") {
                        let user: Uuid = path.rsplit('/').next().unwrap().parse().unwrap();
                        let mut shop = shop.lock().unwrap();
                        shop.premium_reads += 1;
                        if let Some(status) = shop.premium_status {
                            return Response::builder()
                                .status(poem::http::StatusCode::from_u16(status).unwrap())
                                .body("renewal rolled back");
                        }
                        // Backend serializes this under the account lock.
                        if shop.renewal_due.remove(&user) && !shop.renewal_unfunded.remove(&user) {
                            shop.premium.insert(user);
                            shop.renewals += 1;
                        }
                        json!(shop.premium.contains(&user))
                    } else if path.contains("/hearts/") {
                        let user: Uuid = path.rsplit('/').next().unwrap().parse().unwrap();
                        let mut shop = shop.lock().unwrap();
                        shop.heart_reads += 1;
                        json!({"hearts":*shop.balances.entry(user).or_insert(6)})
                    } else {
                        panic!("unexpected local service request: {path}");
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
            Server::new_with_acceptor(acceptor).run(app).await.unwrap();
        });
        let mut config = lib::config::load().unwrap();
        config.database.url = std::env::var("HEART_TEST_DATABASE_URL")
            .unwrap()
            .parse()
            .unwrap();
        config.challenges.learning_access_reads = enabled;
        config.services.shop = format!("http://{address}/shop/").parse().unwrap();
        config.services.auth = format!("http://{address}/auth/").parse().unwrap();
        config.services.skills = format!("http://{address}/skills/").parse().unwrap();
        config.challenges.coding_challenges.sandkasten_url =
            format!("http://{address}/").parse().unwrap();
        config.challenges.multiple_choice_questions.timeout = 0;
        config.challenges.matchings.timeout = 0;
        config.challenges.questions.timeout = 0;
        let cache = Cache::new(
            AsyncRedisBackend::new(redis.clone(), format!("heart-test-{}", Uuid::new_v4())),
            PostcardFormatter,
            Duration::from_secs(1),
        );
        let internal_jwt_secrets =
            InternalJwtSecrets::new(secret.clone(), &HashMap::new()).unwrap();
        let services = Services::from_config(
            &internal_jwt_secrets,
            Duration::from_secs(60),
            &config.services,
            cache.clone(),
        );
        Self {
            state: Arc::new(SharedState {
                jwt_secret: secret,
                internal_jwt_secrets,
                auth_redis: redis,
                services,
                cache,
                db,
            }),
            config: Arc::new(config),
            shop,
            server,
        }
    }

    pub(crate) async fn seed(&self, kind: &str) -> (Uuid, Uuid) {
        self.seed_with_enabled(kind, true).await
    }

    pub(crate) async fn seed_with_enabled(&self, kind: &str, enabled: bool) -> (Uuid, Uuid) {
        let task = Uuid::new_v4();
        let subtask = Uuid::new_v4();
        let author = Uuid::new_v4();
        self.state
            .db
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "INSERT INTO challenges_tasks(id,creator,creation_timestamp) VALUES($1,$2,now())",
                [task.into(), author.into()],
            ))
            .await
            .unwrap();
        self.state.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
            "INSERT INTO challenges_subtasks(id,task_id,creator,creation_timestamp,xp,coins,enabled,retired,ty) VALUES($1,$2,$3,now(),0,5,$5,false,$4::text::challenges_subtask_type)",
            [subtask.into(),task.into(),author.into(),kind.into(),enabled.into()])).await.unwrap();
        let sql = match kind {
            "multiple_choice_question" => "INSERT INTO challenges_multiple_choice_quizes(subtask_id,question,answers,correct_answers,single_choice) VALUES($1,'Choose',ARRAY['yes','no'],1,true)",
            "matching" => "INSERT INTO challenges_matchings(subtask_id,\"left\",\"right\",solution) VALUES($1,ARRAY['a','b'],ARRAY['A','B'],ARRAY[0,1]::smallint[])",
            "question" => "INSERT INTO challenges_questions(subtask_id,question,answers,case_sensitive,ascii_letters,digits,punctuation,blocks) VALUES($1,'Answer',ARRAY['yes'],false,true,true,true,ARRAY[]::text[])",
            "coding_challenge" => "INSERT INTO challenges_coding_challenges(subtask_id,time_limit,memory_limit,evaluator,description,solution_environment,solution_code,static_tests,random_tests) VALUES($1,1000,128,'','code','python','',1,1)",
            _ => panic!("unsupported fixture kind"),
        };
        self.state
            .db
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                sql,
                [subtask.into()],
            ))
            .await
            .unwrap();
        (task, subtask)
    }

    pub(crate) fn app(&self) -> impl Endpoint {
        Route::new()
            .nest(
                "/",
                OpenApiService::new(
                    (
                        super::multiple_choice::MultipleChoice {
                            state: self.state.clone(),
                            config: self.config.clone(),
                        },
                        super::matchings::Matchings {
                            state: self.state.clone(),
                            config: self.config.clone(),
                        },
                        super::question::Questions {
                            state: self.state.clone(),
                            config: self.config.clone(),
                        },
                        super::attempts::Attempts {
                            state: self.state.clone(),
                        },
                    ),
                    "Local heart regression",
                    "1",
                ),
            )
            .with(DbTransactionMiddleware::new(self.state.db.clone()))
            .with(crate::services::hearts::SettlementMiddleware(
                self.state.clone(),
            ))
            .data(self.state.clone())
    }

    pub(crate) async fn call(
        &self,
        app: &impl Endpoint,
        user: Uuid,
        admin: bool,
        method: poem::http::Method,
        path: &str,
        body: Value,
    ) -> (u16, Value) {
        let token = sign_jwt(
            UserAccessToken {
                uid: user,
                rt: Uuid::new_v4().to_string(),
                data: UserAccessTokenData {
                    admin,
                    email_verified: true,
                },
            },
            &self.state.jwt_secret,
            Duration::from_secs(60),
        )
        .unwrap();
        let request = Request::builder()
            .method(method)
            .uri(path.parse().unwrap())
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(body.to_string());
        let mut response = match app.call(request).await {
            Ok(response) => response.into_response(),
            Err(error) => error.into_response(),
        };
        let status = response.status().as_u16();
        let bytes = response.take_body().into_bytes().await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| json!({"raw":String::from_utf8_lossy(&bytes)})),
        )
    }
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn heart_routes_postgres() {
    let f = Fixture::new().await;
    let app = f.app();
    for (kind, route, correct, wrong) in [
        (
            "multiple_choice_question",
            "multiple_choice",
            json!({"answers":[true,false]}),
            json!({"answers":[false,true]}),
        ),
        (
            "matching",
            "matchings",
            json!({"answer":[0,1]}),
            json!({"answer":[1,0]}),
        ),
        (
            "question",
            "questions",
            json!({"answer":"yes"}),
            json!({"answer":"no"}),
        ),
    ] {
        let user = Uuid::new_v4();
        let (task, subtask) = f.seed(kind).await;
        let path = format!("/tasks/{task}/{route}/{subtask}/attempts");
        let mut ids = HashSet::new();
        for (body, solved, balance) in [
            (correct.clone(), true, 6),
            (correct.clone(), true, 6),
            (wrong.clone(), false, 4),
            (wrong.clone(), false, 2),
        ] {
            let (status, result) = f
                .call(&app, user, false, poem::http::Method::POST, &path, body)
                .await;
            assert_eq!(status, 201, "{route}: {result}");
            assert_eq!(result["solved"], solved);
            assert_eq!(result["hearts_pending"], false);
            assert_eq!(
                *f.shop.lock().unwrap().balances.get(&user).unwrap(),
                balance
            );
            let id = result["attempt_id"].as_str().unwrap();
            assert!(ids.insert(id.to_owned()));
            let (status, proof) = f
                .call(
                    &app,
                    user,
                    false,
                    poem::http::Method::GET,
                    &format!("{path}/{id}"),
                    json!(null),
                )
                .await;
            assert_eq!(status, 200);
            assert_eq!(proof["id"], id);
            assert_eq!(proof["solved"], solved);
            assert_eq!(proof["user_id"], json!(user));
            assert_eq!(proof["task_id"], json!(task));
            assert_eq!(proof["subtask_id"], json!(subtask));
            chrono::DateTime::parse_from_rfc3339(proof["created_at"].as_str().unwrap()).unwrap();
            assert_eq!(
                f.call(
                    &app,
                    Uuid::new_v4(),
                    true,
                    poem::http::Method::GET,
                    &format!("{path}/{id}"),
                    json!(null)
                )
                .await
                .0,
                404
            );
        }
        let row = f.state.db.query_one(Statement::from_sql_and_values(DbBackend::Postgres,
            "SELECT count(*) AS n FROM challenge_benefit_earnings WHERE user_id=$1 AND subtask_id=$2",[user.into(),subtask.into()])).await.unwrap().unwrap();
        assert_eq!(row.try_get::<i64>("", "n").unwrap(), 1);
        f.shop.lock().unwrap().balances.insert(user, 1);
        assert_eq!(
            f.call(
                &app,
                user,
                false,
                poem::http::Method::POST,
                &path,
                correct.clone()
            )
            .await
            .0,
            403
        );
        f.shop.lock().unwrap().premium.insert(user);
        assert_eq!(
            f.call(
                &app,
                user,
                false,
                poem::http::Method::POST,
                &path,
                wrong.clone()
            )
            .await
            .0,
            201
        );
        assert_eq!(f.shop.lock().unwrap().balances[&user], 1);
    }
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn heart_lost_reply_and_parallel_settlement_postgres() {
    let f = Fixture::new().await;
    let app = f.app();
    let user = Uuid::new_v4();
    let (task, subtask) = f.seed("multiple_choice_question").await;
    f.shop.lock().unwrap().lose_reply = true;
    let (status, result) = f
        .call(
            &app,
            user,
            false,
            poem::http::Method::POST,
            &format!("/tasks/{task}/multiple_choice/{subtask}/attempts"),
            json!({"answers":[false,true]}),
        )
        .await;
    assert_eq!(status, 201);
    assert_eq!(result["solved"], false);
    assert_eq!(result["hearts_pending"], true);
    let operation: Uuid = result["attempt_id"].as_str().unwrap().parse().unwrap();
    assert_eq!(f.shop.lock().unwrap().balances[&user], 4);
    let (a, b) = tokio::join!(
        crate::services::hearts::settle(&f.state.db, &f.state.services, operation),
        crate::services::hearts::settle(&f.state.db, &f.state.services, operation)
    );
    assert!(a.unwrap() && b.unwrap());
    assert_eq!(f.shop.lock().unwrap().balances[&user], 4);
    assert_eq!(f.shop.lock().unwrap().calls, 2); // original plus one exact replay
    let abandoned = Uuid::new_v4();
    let tx = f.state.db.begin().await.unwrap();
    crate::services::hearts::record(&tx, abandoned, user, subtask)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert!(
        crate::services::hearts::settle(&f.state.db, &f.state.services, abandoned)
            .await
            .unwrap()
    );
    assert_eq!(f.shop.lock().unwrap().calls, 2);
    let tx = f.state.db.begin().await.unwrap();
    let export = crate::services::users::export_user_data(&tx, user)
        .await
        .unwrap();
    assert_eq!(export.heart_operations.as_array().unwrap().len(), 1);
    crate::services::users::delete_user_data(&tx, user)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(f
        .state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT 1 FROM challenge_heart_operations WHERE user_id=$1",
            [user.into()]
        ))
        .await
        .unwrap()
        .is_none());
}

async fn wrong_answer(f: &Fixture, app: &impl Endpoint, path: &str, user: Uuid) -> (u16, Value) {
    f.call(
        app,
        user,
        false,
        poem::http::Method::POST,
        path,
        json!({"answers":[false,true]}),
    )
    .await
}

async fn user_rows(f: &Fixture, table: &str, user: Uuid) -> i64 {
    f.state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            format!("SELECT count(*) AS n FROM {table} WHERE user_id=$1"),
            [user.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap()
}

#[tokio::test]
#[ignore = "requires explicitly supplied disposable PostgreSQL and Redis"]
async fn premium_renewal_due_during_open_exercise_postgres() {
    let f = Fixture::new().await;
    let app = f.app();
    let (task, subtask) = f.seed("multiple_choice_question").await;
    let path = format!("/tasks/{task}/multiple_choice/{subtask}/attempts");
    let attempts = "challenges_multiple_choice_attempts";
    let debits = "challenge_heart_operations";

    // Paid time covers the start of the exercise.
    let user = Uuid::new_v4();
    f.shop.lock().unwrap().premium.insert(user);
    let (status, body) = wrong_answer(&f, &app, &path, user).await;
    assert_eq!(status, 201, "{body}");
    // It ends while the exercise is open; the confirmed renewal is due. Two
    // answers arrive at once: both are admitted, the renewal settles once.
    {
        let mut shop = f.shop.lock().unwrap();
        shop.premium.remove(&user);
        shop.renewal_due.insert(user);
    }
    let (a, b) = tokio::join!(
        wrong_answer(&f, &app, &path, user),
        wrong_answer(&f, &app, &path, user)
    );
    for (status, body) in [a, b] {
        assert_eq!(status, 201, "{body}");
        assert_eq!(body["solved"], false);
        assert_eq!(body["hearts_pending"], false);
    }
    {
        let shop = f.shop.lock().unwrap();
        assert_eq!((shop.renewals, shop.premium_reads), (1, 1));
        assert_eq!(shop.calls, 0);
    }
    assert_eq!(user_rows(&f, attempts, user).await, 3);
    assert_eq!(user_rows(&f, debits, user).await, 0);

    // Settlement fails before Backend commits: retryable 503, nothing saved.
    let failed = Uuid::new_v4();
    {
        let mut shop = f.shop.lock().unwrap();
        shop.renewal_due.insert(failed);
        shop.premium_status = Some(500);
    }
    let (status, body) = wrong_answer(&f, &app, &path, failed).await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["code"], "learning_access_unavailable");
    assert_eq!(user_rows(&f, attempts, failed).await, 0);
    assert_eq!(user_rows(&f, debits, failed).await, 0);
    {
        let mut shop = f.shop.lock().unwrap();
        assert!(shop.renewal_due.contains(&failed));
        assert_eq!(shop.renewals, 1);
        shop.premium_status = None;
    }
    // The retried answer settles once and costs no heart.
    let (status, body) = wrong_answer(&f, &app, &path, failed).await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(f.shop.lock().unwrap().renewals, 2);
    assert_eq!(user_rows(&f, attempts, failed).await, 1);
    assert_eq!(user_rows(&f, debits, failed).await, 0);

    // The coins no longer cover the renewal: Backend ends the agreement and
    // the wrong answer follows the ordinary heart rule.
    let declined = Uuid::new_v4();
    {
        let mut shop = f.shop.lock().unwrap();
        shop.renewal_due.insert(declined);
        shop.renewal_unfunded.insert(declined);
    }
    let (status, body) = wrong_answer(&f, &app, &path, declined).await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["hearts_pending"], false);
    {
        let shop = f.shop.lock().unwrap();
        assert_eq!(shop.renewals, 2);
        assert!(!shop.renewal_due.contains(&declined));
        assert_eq!(shop.balances[&declined], 4);
    }
    assert_eq!(user_rows(&f, debits, declined).await, 1);

    // Other policy failures stay a retryable outage. Only a 500 triggers the
    // settlement read, and a positive Premium read alone never admits.
    let outage = Uuid::new_v4();
    f.shop.lock().unwrap().premium.insert(outage);
    for (status, reads) in [(500, 1), (503, 0)] {
        let before = {
            let mut shop = f.shop.lock().unwrap();
            shop.policy_status = Some(status);
            shop.premium_reads
        };
        let (code, body) = wrong_answer(&f, &app, &path, outage).await;
        assert_eq!(code, 503, "{body}");
        assert_eq!(f.shop.lock().unwrap().premium_reads, before + reads);
    }
    f.shop.lock().unwrap().policy_status = None;
    assert_eq!(user_rows(&f, attempts, outage).await, 0);
}
