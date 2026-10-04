//! Actual six routes, real migrated SQL/Valkey and a synthetic authority.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use fnct::{
    backend::AsyncRedisBackend,
    format::{JsonFormatter, PostcardFormatter},
};
use lib::{
    config,
    jwt::{
        sign_jwt, verify_jwt, InternalAuthToken, InternalJwtSecrets, JwtSecret, UserAccessToken,
        UserAccessTokenData,
    },
    redis::RedisConnection,
    services::{
        publications::{
            PublicationEpoch, PublicationParticipant, PublicationSnapshot, SCOPE_VERSION,
        },
        Services,
    },
    Cache, SharedState,
};
use poem::{
    endpoint::make,
    listener::{Acceptor, Listener, TcpListener},
    Endpoint, EndpointExt, IntoResponse, Request, Response, Route, Server,
};
use poem_ext::db::DbTransactionMiddleware;
use poem_openapi::OpenApiService;
use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
use serde_json::{json, Value};
use uuid::Uuid;

use super::leaderboard::LeaderboardEndpoints;
use crate::services::leaderboard::published;

struct Authority {
    snapshot: PublicationSnapshot,
    scores: Vec<(Uuid, u64)>,
    private: Uuid,
    identities: usize,
    epochs: usize,
    snapshots: usize,
    bad_epoch: bool,
    bad_snapshot: bool,
    outage: bool,
    revoke_after_snapshot: Option<Uuid>,
}

impl Authority {
    fn revoke(&mut self, id: Uuid) {
        self.snapshot
            .participants
            .retain(|person| person.user_id != id);
        self.bump();
    }
    fn bump(&mut self) {
        self.snapshot.epoch.publication_epoch = Uuid::new_v4();
        self.snapshot.epoch.epoch_revision += 1;
    }
    fn rows(&self, shared: bool) -> Vec<Value> {
        let mut scores = self.scores.clone();
        if shared {
            scores.retain(|(id, _)| self.snapshot.participant(*id).is_some());
        }
        scores.sort_by_key(|(id, score)| (std::cmp::Reverse(*score), *id));
        scores
            .iter()
            .map(|(id, score)| {
                json!({"user":id,"xp":score,
            "rank":1 + scores.iter().filter(|(_, other)| other > score).count()})
            })
            .collect()
    }
}

struct Fixture {
    state: Arc<SharedState>,
    authority: Arc<Mutex<Authority>>,
    server: tokio::task::JoinHandle<()>,
    ids: Vec<Uuid>,
    task: Uuid,
    subtasks: Vec<Uuid>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Fixture {
    async fn new() -> Self {
        let db =
            Database::connect(std::env::var("PRIV01_TEST_DATABASE_URL").expect("own migrated PG"))
                .await
                .unwrap();
        let redis =
            RedisConnection::new(&std::env::var("PRIV01_TEST_REDIS_URL").expect("own Valkey"))
                .await
                .unwrap();
        let secret = JwtSecret::try_from("synthetic-publication-rankings").unwrap();
        let ids: Vec<_> = (0..6).map(|_| Uuid::new_v4()).collect();
        let authority = Arc::new(Mutex::new(Authority {
            snapshot: PublicationSnapshot {
                epoch: PublicationEpoch {
                    scope_version: SCOPE_VERSION.into(),
                    publication_epoch: Uuid::new_v4(),
                    epoch_revision: 1,
                    policy_active: false,
                    publishing_enabled: false,
                },
                participants: ids[1..]
                    .iter()
                    .enumerate()
                    .map(|(index, id)| PublicationParticipant {
                        user_id: *id,
                        visibility_revision: 1,
                        display_name: format!("Shared {}", index + 1),
                        avatar_url: Value::Null,
                    })
                    .collect(),
            },
            scores: ids.iter().copied().zip([900, 30, 20, 20, 10, 0]).collect(),
            private: ids[0],
            identities: 0,
            epochs: 0,
            snapshots: 0,
            bad_epoch: false,
            bad_snapshot: false,
            outage: false,
            revoke_after_snapshot: None,
        }));
        let app = make({
            let authority = authority.clone();
            let secret = secret.clone();
            move |mut request: Request| {
                let authority = authority.clone();
                let secret = secret.clone();
                async move {
                    let path = request.uri().path().to_owned();
                    if path.ends_with("ordinary-authority") {
                        let body: Value = request.take_body().into_json().await.unwrap();
                        let user: UserAccessToken =
                            verify_jwt(body["access_token"].as_str().unwrap(), &secret).unwrap();
                        return Response::builder().content_type("application/json").body(
                            json!({"id":user.uid,
                            "admin":user.data.admin,"email_verified":user.data.email_verified})
                            .to_string(),
                        );
                    }
                    let bearer = request.headers()["Authorization"]
                        .to_str()
                        .unwrap()
                        .trim_start_matches("Bearer ");
                    let token: InternalAuthToken = verify_jwt(bearer, &secret).unwrap();
                    assert!(token.aud == "auth" || token.aud == "skills");
                    let mut state = authority.lock().unwrap();
                    if state.outage && path.contains("profile-publications") {
                        return Response::builder()
                            .status(poem::http::StatusCode::SERVICE_UNAVAILABLE)
                            .body("unavailable");
                    }
                    let value = if path.ends_with("profile-publications/epoch") {
                        state.epochs += 1;
                        let mut epoch = serde_json::to_value(&state.snapshot.epoch).unwrap();
                        if state.bad_epoch {
                            epoch.as_object_mut().unwrap().remove("policy_active");
                        }
                        epoch
                    } else if path.ends_with("profile-publications/snapshot") {
                        state.snapshots += 1;
                        let mut snapshot = serde_json::to_value(&state.snapshot).unwrap();
                        if state.bad_snapshot {
                            snapshot["participants"][0]
                                .as_object_mut()
                                .unwrap()
                                .remove("display_name");
                        }
                        if let Some(id) = state.revoke_after_snapshot.take() {
                            state.revoke(id);
                        }
                        snapshot
                    } else if path.contains("/users/") {
                        state.identities += 1;
                        let id: Uuid = path.rsplit('/').next().unwrap().parse().unwrap();
                        if !state.ids().contains(&id) {
                            return Response::builder()
                                .status(poem::http::StatusCode::NOT_FOUND)
                                .body("unknown");
                        }
                        json!({"id":id,"name":"login-must-stay-private","display_name":"Legacy", "avatar_url":null,
                            "registration":12,"admin":false,"leaderboard_opt_out":id == state.private})
                    } else if path.contains("leaderboard") {
                        let params: HashMap<_, _> =
                            request.uri().query().map(url_pairs).unwrap_or_default();
                        let shared = path.contains("published-leaderboard");
                        if shared
                            && params.get("publication_epoch")
                                != Some(&state.snapshot.epoch.publication_epoch.to_string())
                        {
                            return Response::builder()
                                .status(poem::http::StatusCode::CONFLICT)
                                .body("epoch changed");
                        }
                        let rows = state.rows(shared);
                        let tail = path.rsplit('/').next().unwrap();
                        let mut value = if let Ok(id) = tail.parse::<Uuid>() {
                            let xp = state
                                .scores
                                .iter()
                                .find(|(user, _)| *user == id)
                                .map(|(_, xp)| *xp)
                                .unwrap_or(0);
                            let rank = rows
                                .iter()
                                .find(|row| row["user"] == id.to_string())
                                .map(|row| row["rank"].clone())
                                .unwrap_or(Value::Null);
                            if shared {
                                json!({"xp":xp,"rank":rank,"public_rank":rank})
                            } else {
                                json!({"xp":xp,"rank":1+state.scores.iter().filter(|(_,other)| *other > xp).count()})
                            }
                        } else {
                            let limit: usize = params["limit"].parse().unwrap();
                            let offset: usize = params["offset"].parse().unwrap();
                            json!({"total":rows.len(),"leaderboard":rows.into_iter().skip(offset).take(limit).collect::<Vec<_>>()})
                        };
                        if shared {
                            value["scope_version"] = json!(state.snapshot.epoch.scope_version);
                            value["publication_epoch"] =
                                json!(state.snapshot.epoch.publication_epoch);
                            value["epoch_revision"] = json!(state.snapshot.epoch.epoch_revision);
                        }
                        value
                    } else {
                        panic!("unexpected local service path {path}");
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
        let origin = format!(
            "http://{}/",
            acceptor.local_addr()[0].as_socket_addr().unwrap()
        );
        let server = tokio::spawn(async move {
            Server::new_with_acceptor(acceptor).run(app).await.unwrap();
        });
        let mut config = config::load().unwrap();
        config.services.auth = origin.parse().unwrap();
        config.services.skills = origin.parse().unwrap();
        let secrets = InternalJwtSecrets::new(secret.clone(), &HashMap::new()).unwrap();
        let cache = Cache::new(
            AsyncRedisBackend::new(
                redis.clone(),
                format!("publication-test-{}", Uuid::new_v4()),
            ),
            PostcardFormatter,
            Duration::from_secs(300),
        );
        let services = Services::from_config(
            &secrets,
            Duration::from_secs(60),
            &config.services,
            cache.clone(),
        );
        let state = Arc::new(SharedState {
            jwt_secret: secret,
            internal_jwt_secrets: secrets,
            auth_redis: redis,
            services,
            cache,
            db,
        });
        let task = Uuid::new_v4();
        state
            .db
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "INSERT INTO challenges_tasks(id,creator,creation_timestamp) VALUES($1,$2,now())",
                [task.into(), ids[0].into()],
            ))
            .await
            .unwrap();
        let mut subtasks = Vec::new();
        for (id, xp) in ids.iter().zip([900, 30, 20, 20, 10, 0]) {
            let subtask = Uuid::new_v4();
            let submission = Uuid::new_v4();
            subtasks.push(subtask);
            state.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
                "INSERT INTO challenges_subtasks(id,task_id,creator,creation_timestamp,xp,coins,enabled,retired,ty) VALUES($1,$2,$3,now(),$4,0,true,false,'coding_challenge')",
                vec![subtask.into(),task.into(),ids[0].into(),xp.into()])).await.unwrap();
            state.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
                "INSERT INTO challenges_coding_challenges(subtask_id,time_limit,memory_limit,evaluator,description,solution_environment,solution_code,static_tests,random_tests) VALUES($1,1000,128,'','private','python','',1,1)", [subtask.into()])).await.unwrap();
            state.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
                "INSERT INTO challenges_user_subtasks(user_id,subtask_id,solved_timestamp,attempts) VALUES($1,$2,'2026-01-01',1)", [(*id).into(),subtask.into()])).await.unwrap();
            state.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
                "INSERT INTO challenges_coding_challenge_submissions(id,subtask_id,creator,creation_timestamp,environment,code,judge_pending) VALUES($1,$2,$3,'2026-01-01','python','private solution',false)", [submission.into(),subtask.into(),(*id).into()])).await.unwrap();
            state.db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
                "INSERT INTO challenges_coding_challenge_result(submission_id,verdict) VALUES($1,'ok')", [submission.into()])).await.unwrap();
        }
        Self {
            state,
            authority,
            server,
            ids,
            task,
            subtasks,
        }
    }
    fn app(&self, enabled: bool) -> impl Endpoint {
        let mut config = config::load().unwrap();
        config.challenges.profile_publications_enabled = enabled;
        Route::new()
            .nest(
                "/",
                OpenApiService::new(
                    LeaderboardEndpoints {
                        state: self.state.clone(),
                        cache: self.state.cache.with_formatter(JsonFormatter),
                        config: Arc::new(config),
                    },
                    "Publication fixtures",
                    "1",
                ),
            )
            .with(DbTransactionMiddleware::new(self.state.db.clone()))
            .with(published::Headers(enabled))
            .data(self.state.clone())
    }
    async fn call(
        &self,
        app: &impl Endpoint,
        path: &str,
        viewer: Option<(Uuid, bool, bool)>,
    ) -> (u16, Value) {
        let mut request = Request::builder()
            .uri(path.parse().unwrap())
            .header("If-None-Match", "*");
        if let Some((id, admin, verified)) = viewer {
            let token = sign_jwt(
                UserAccessToken {
                    uid: id,
                    rt: Uuid::new_v4().to_string(),
                    data: UserAccessTokenData {
                        admin,
                        email_verified: verified,
                    },
                },
                &self.state.jwt_secret,
                Duration::from_secs(60),
            )
            .unwrap();
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        let response = app.call(request.finish()).await.unwrap().into_response();
        let status = response.status().as_u16();
        if self.authority.lock().unwrap().snapshot.epoch.policy_active {
            assert_eq!(response.headers()["Cache-Control"], "private, no-store");
            assert_eq!(response.headers()["Vary"], "Authorization");
        }
        let body: Value = response.into_body().into_json().await.unwrap();
        if status == 200 && body.get("leaderboard").is_some() {
            let wire = body.to_string();
            for id in &self.ids {
                assert!(
                    !wire.contains(&id.to_string()),
                    "account ID escaped in a leaderboard"
                );
            }
        }
        (status, body)
    }
    fn lists(&self) -> Vec<String> {
        vec![
            "/leaderboard".into(),
            format!("/leaderboard/by-task/{}", self.task),
            "/leaderboard/by-language/python".into(),
        ]
    }
}

impl Authority {
    fn ids(&self) -> Vec<Uuid> {
        self.scores.iter().map(|(id, _)| *id).collect()
    }
}
fn url_pairs(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .map(|v| {
            let (k, v) = v.split_once('=').unwrap();
            (k.into(), v.into())
        })
        .collect()
}

#[tokio::test]
#[ignore = "Requires owned migrated PostgreSQL and Valkey"]
async fn publication_all_six_routes_sql_caches_and_revocation() {
    let f = Fixture::new().await;
    let owner = Some((f.ids[0], false, true));
    let legacy = f.app(false);
    // Warm the exact legacy namespaces and verify all six old route shapes.
    for route in f.lists() {
        let (status, page) = f
            .call(&legacy, &format!("{route}?limit=2&offset=0"), owner)
            .await;
        assert_eq!(status, 200);
        assert_eq!(page["total"], 6);
        assert_eq!(page["leaderboard"].as_array().unwrap().len(), 1);
        assert_eq!(page["leaderboard"][0]["rank"], 2);
        assert_eq!(
            page["leaderboard"][0]["user"]["name"],
            "login-must-stay-private"
        );
        assert!(page.get("publication_epoch").is_none());
        let (status, rank) = f
            .call(&legacy, &format!("{route}/{}", f.ids[0]), owner)
            .await;
        assert_eq!((status, rank), (200, json!({"score":900,"rank":1})));
    }
    {
        let mut a = f.authority.lock().unwrap();
        a.snapshot.epoch.policy_active = true;
        a.snapshot.epoch.publishing_enabled = true;
        a.bump();
    }
    let active = f.app(true);
    let calls_before = f.authority.lock().unwrap().identities;
    for route in f.lists() {
        let (status, page) = f
            .call(&active, &format!("{route}?limit=2&offset=0"), owner)
            .await;
        assert_eq!(status, 200);
        assert_eq!(page["total"], 5);
        assert_eq!(page["leaderboard"].as_array().unwrap().len(), 2);
        assert_eq!(page["leaderboard"][0]["score"], 30);
        assert_eq!(page["leaderboard"][0]["rank"], 1);
        assert_eq!(page["leaderboard"][1]["rank"], 2);
        let user = page["leaderboard"][0]["user"].as_object().unwrap();
        assert_eq!(user.len(), 2);
        assert!(user.contains_key("display_name") && user["avatar_url"].is_null());
        let epoch = page["publication_epoch"].as_str().unwrap();
        let (status, next) = f
            .call(
                &active,
                &format!("{route}?limit=2&offset=2&publication_epoch={epoch}"),
                owner,
            )
            .await;
        assert_eq!(status, 200);
        assert_eq!(next["leaderboard"][0]["rank"], 2);
        assert_eq!(next["leaderboard"][1]["rank"], 4);
        let (status, own) = f
            .call(&active, &format!("{route}/{}", f.ids[0]), owner)
            .await;
        assert_eq!(status, 200);
        assert_eq!(own["score"], 900);
        assert!(own["public_rank"].is_null() && own["rank"].is_null());
        for admin in [false, true] {
            let viewer = Some((f.ids[1], admin, true));
            let private = f
                .call(&active, &format!("{route}/{}", f.ids[0]), viewer)
                .await;
            let absent = f
                .call(&active, &format!("{route}/{}", Uuid::new_v4()), viewer)
                .await;
            assert_eq!(private, absent);
            assert_eq!(private, (404, json!({"error":"not_found"})));
        }
        let (status, rank) = f
            .call(&active, &format!("{route}/{}", f.ids[1]), owner)
            .await;
        assert_eq!(status, 200);
        assert_eq!(rank["public_rank"], 1);
        for viewer in [
            None,
            Some((f.ids[1], false, false)),
            Some((f.ids[1], true, false)),
        ] {
            assert!(matches!(
                f.call(&active, &format!("{route}?limit=2&offset=0"), viewer)
                    .await
                    .0,
                401 | 403
            ));
        }
    }
    assert_eq!(
        f.authority.lock().unwrap().identities,
        calls_before,
        "shared mode must never use per-user identity RPCs"
    );
    // Private XP and task-score changes leave every public byte unchanged.
    for route in f.lists() {
        let (_, before) = f
            .call(&active, &format!("{route}?limit=100&offset=0"), owner)
            .await;
        f.state
            .db
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "UPDATE challenges_subtasks SET xp=99000 WHERE id=$1",
                [f.subtasks[0].into()],
            ))
            .await
            .unwrap();
        f.authority.lock().unwrap().scores[0].1 = 99000;
        let (_, after) = f
            .call(&active, &format!("{route}?limit=100&offset=0"), owner)
            .await;
        assert_eq!(before, after);
    }
    // Fresh score content invalidates a warm active cache without an epoch event.
    f.state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "UPDATE challenges_subtasks SET xp=50 WHERE id=$1",
            [f.subtasks[4].into()],
        ))
        .await
        .unwrap();
    f.authority.lock().unwrap().scores[4].1 = 50;
    for route in f.lists() {
        let (_, page) = f
            .call(&active, &format!("{route}?limit=100&offset=0"), owner)
            .await;
        assert_eq!(page["leaderboard"][0]["user"]["display_name"], "Shared 4");
        assert_eq!(page["leaderboard"][0]["score"], 50);
    }
    let old_epoch = f.authority.lock().unwrap().snapshot.epoch.publication_epoch;
    f.authority.lock().unwrap().revoke(f.ids[4]);
    for route in f.lists() {
        let (_, page) = f
            .call(&active, &format!("{route}?limit=100&offset=0"), owner)
            .await;
        assert_eq!(page["total"], 4);
        assert_eq!(page["leaderboard"][0]["score"], 30);
        assert_eq!(page["leaderboard"][0]["rank"], 1);
        assert_eq!(
            f.call(
                &active,
                &format!("{route}?limit=2&offset=2&publication_epoch={old_epoch}"),
                owner
            )
            .await
            .0,
            409
        );
    }
    // Revoke while a populated old snapshot is in flight on all six routes.
    let before_race = f.authority.lock().unwrap().snapshot.participants.clone();
    for route in f.lists() {
        for single_rank in [false, true] {
            {
                let mut a = f.authority.lock().unwrap();
                a.snapshot.participants.clone_from(&before_race);
                a.bump();
                a.revoke_after_snapshot = Some(f.ids[1]);
            }
            let path = if single_rank {
                format!("{route}/{}", f.ids[1])
            } else {
                format!("{route}?limit=100&offset=0")
            };
            let (status, value) = f.call(&active, &path, owner).await;
            if single_rank {
                assert_eq!((status, value), (404, json!({"error":"not_found"})));
            } else {
                assert_eq!(status, 200);
                assert_eq!(value["total"], 3);
                assert_eq!(value["leaderboard"][0]["score"], 20);
                assert_eq!(value["leaderboard"][0]["rank"], 1);
            }
        }
    }
    // Failed/old authority and rollback never select the old warm public caches.
    for state in [
        "outage",
        "missing_epoch",
        "missing_identity",
        "disabled_backend",
        "disabled_reader",
        "old_scope",
    ] {
        {
            let mut a = f.authority.lock().unwrap();
            a.outage = state == "outage";
            a.bad_epoch = state == "missing_epoch";
            a.bad_snapshot = state == "missing_identity";
            a.snapshot.epoch.publishing_enabled = state != "disabled_backend";
            a.snapshot.epoch.scope_version = if state == "old_scope" {
                "old".into()
            } else {
                SCOPE_VERSION.into()
            };
        }
        let app = f.app(state != "disabled_reader");
        for route in f.lists() {
            assert_eq!(
                f.call(&app, &format!("{route}?limit=2&offset=0"), owner)
                    .await
                    .0,
                503
            );
            assert_eq!(
                f.call(&app, &format!("{route}/{}", f.ids[1]), owner)
                    .await
                    .0,
                503
            );
        }
    }
    {
        let mut a = f.authority.lock().unwrap();
        a.snapshot.epoch.scope_version = SCOPE_VERSION.into();
        a.snapshot.participants.clear();
        a.bump();
    }
    for route in f.lists() {
        let (status, page) = f
            .call(&active, &format!("{route}?limit=2&offset=0"), owner)
            .await;
        assert_eq!(status, 200);
        assert_eq!(page["total"], 0);
        assert_eq!(page["leaderboard"], json!([]));
    }
}
