#![forbid(unsafe_code)]
#![warn(clippy::dbg_macro, clippy::use_debug)]

use std::{sync::Arc, time::Duration};

use fnct::async_redis::AsyncRedisCache;
use lib::{config, jwt::JwtSecret, redis::RedisConnection, services::Services, SharedState};
use poem::{listener::TcpListener, middleware::Tracing, EndpointExt, Route, Server};
use poem_ext::panic_handler::PanicHandler;
use poem_openapi::OpenApiService;
use sea_orm::{ConnectOptions, Database};
use tracing::info;

use crate::endpoints::get_api;

mod endpoints;
mod schemas;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    info!("Loading config");
    let config = config::load()?;

    info!("Connecting to database");
    let mut db_options = ConnectOptions::new(config.database.url.into());
    db_options.connect_timeout(Duration::from_secs(config.database.connect_timeout));
    let db = Database::connect(db_options).await?;

    info!("Connecting to redis");
    let cache = AsyncRedisCache::new(
        RedisConnection::new(config.redis.challenges.as_str()).await?,
        "challenges".into(),
        Duration::from_secs(config.cache_ttl),
    );
    let auth_redis = RedisConnection::new(config.redis.auth.as_str()).await?;

    let jwt_secret = JwtSecret::try_from(config.jwt_secret.as_str())?;
    let services = Services::from_config(
        jwt_secret.clone(),
        Duration::from_secs(config.internal_jwt_ttl),
        config.services,
        cache,
    );
    let shared_state = Arc::new(SharedState {
        db,
        jwt_secret,
        auth_redis,
        services,
    });

    let api_service = OpenApiService::new(
        get_api(shared_state.clone()),
        "Bootstrap Academy Backend: Challenges Microservice",
        env!("CARGO_PKG_VERSION"),
    )
    .external_document("/openapi.json");
    let app = Route::new()
        .nest("/openapi.json", api_service.spec_endpoint())
        .nest("/docs", api_service.swagger_ui())
        .nest("/redoc", api_service.redoc())
        .nest("/", api_service)
        .with(Tracing)
        .with(PanicHandler::middleware())
        .data(shared_state);

    info!(
        "Listening on {}:{}",
        config.challenges.host, config.challenges.port
    );
    Server::new(TcpListener::bind((
        config.challenges.host,
        config.challenges.port,
    )))
    .run(app)
    .await?;

    Ok(())
}