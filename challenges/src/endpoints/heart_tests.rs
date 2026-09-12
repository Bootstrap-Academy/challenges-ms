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
                    let value = if path.ends_with("/ordinary-authority") {
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
                        let receipt = if let Some(receipt) = shop.receipts.get(&operation) {
                            receipt.clone()
                        } else {
                            let balance = *shop.balances.entry(user).or_insert(6);
                            let outcome = if shop.premium.contains(&user) {
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
                        receipt
                    } else if path.contains("/premium/") {
                        let user: Uuid = path.rsplit('/').next().unwrap().parse().unwrap();
                        json!(shop.lock().unwrap().premium.contains(&user))
                    } else if path.contains("/hearts/") {
                        let user: Uuid = path.rsplit('/').next().unwrap().parse().unwrap();
                        json!({"hearts":*shop.lock().unwrap().balances.entry(user).or_insert(6)})
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
        config.services.shop = format!("http://{address}/shop/").parse().unwrap();
        config.services.auth = format!("http://{address}/auth/").parse().unwrap();
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
            "INSERT INTO challenges_subtasks(id,task_id,creator,creation_timestamp,xp,coins,enabled,retired,ty) VALUES($1,$2,$3,now(),0,5,true,false,$4::text::challenges_subtask_type)",
            [subtask.into(),task.into(),author.into(),kind.into()])).await.unwrap();
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
