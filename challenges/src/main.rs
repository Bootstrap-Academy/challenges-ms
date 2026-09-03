#![forbid(unsafe_code)]
#![warn(clippy::dbg_macro, clippy::use_debug, clippy::todo)]

use std::{env, sync::Arc, time::Duration};

use anyhow::bail;
use fnct::{backend::AsyncRedisBackend, format::PostcardFormatter};
use lib::{
    config::{self, Config},
    jwt::JwtSecret,
    redis::RedisConnection,
    services::Services,
    Cache, SharedState,
};
use poem::{listener::TcpListener, middleware::Tracing, EndpointExt, Route, Server};
use poem_ext::{db::DbTransactionMiddleware, panic_handler::PanicHandler};
use poem_openapi::OpenApiService;
use sandkasten_client::SandkastenClient;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use sentry::integrations::tracing::EventFilter;
use tracing::{info, warn, Level};
use tracing_subscriber::{prelude::*, EnvFilter};

use crate::{endpoints::setup_api, sweep::sweep_deleted_users};

mod endpoints;
mod services;
mod sweep;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Arc::new(config::load()?);

    let _sentry_guard = config.challenges.sentry.as_ref().map(|sentry_config| {
        sentry::init((
            sentry_config.dsn.as_str(),
            sentry::ClientOptions {
                release: Some(env!("CARGO_PKG_VERSION").into()),
                attach_stacktrace: true,
                ..Default::default()
            },
        ))
    });

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(EnvFilter::from_default_env()))
        .with(
            sentry::integrations::tracing::layer().event_filter(|md| match md.level() {
                &Level::ERROR => EventFilter::Exception,
                &Level::WARN => EventFilter::Event,
                &Level::INFO | &Level::DEBUG => EventFilter::Breadcrumb,
                &Level::TRACE => EventFilter::Ignore,
            }),
        )
        .init();

    match env::args().nth(1) {
        None => serve(config).await,
        Some(cmd) if cmd == "sweep-deleted-users" => run_sweep(config).await,
        Some(cmd) => bail!("Unknown subcommand: {cmd}"),
    }
}

async fn serve(config: Arc<Config>) -> anyhow::Result<()> {
    let db = connect_database(&config).await?;
    let cache = connect_cache(&config).await?;
    let auth_redis = RedisConnection::new(config.redis.auth.as_str()).await?;

    info!("Connecting to Sandkasten");
    let sandkasten =
        SandkastenClient::new(config.challenges.coding_challenges.sandkasten_url.clone());
    let server_version = sandkasten.version().await?;
    let client_version = sandkasten_client::VERSION;
    info!("Connected to Sandkasten v{server_version}");
    if server_version != client_version {
        warn!(
            "Sandkasten server version ({server_version}) and client version ({client_version}) \
             differ!"
        );
    }

    let jwt_secret = JwtSecret::try_from(config.jwt_secret.as_str())?;
    let services = Services::from_config(
        jwt_secret.clone(),
        Duration::from_secs(config.internal_jwt_ttl),
        &config.services,
        cache.clone(),
    );
    let shared_state = Arc::new(SharedState {
        jwt_secret,
        auth_redis,
        services,
        cache,
        db: db.clone(),
    });

    let api_service = OpenApiService::new(
        setup_api(shared_state.clone(), Arc::clone(&config), sandkasten).await?,
        "Bootstrap Academy Backend: Challenges Microservice",
        env!("CARGO_PKG_VERSION"),
    )
    .external_document("/openapi.json")
    .server(config.challenges.server.to_string());
    let app = Route::new()
        .nest("/openapi.json", api_service.spec_endpoint())
        .nest("/docs", api_service.swagger_ui())
        .nest("/redoc", api_service.redoc())
        .nest("/", api_service)
        .with(Tracing)
        .with(PanicHandler::middleware())
        .with(DbTransactionMiddleware::new(db))
        .data(shared_state);

    info!(
        "Listening on {}:{}",
        config.challenges.host, config.challenges.port
    );
    Server::new(TcpListener::bind((
        config.challenges.host.as_str(),
        config.challenges.port,
    )))
    .run(app)
    .await?;

    Ok(())
}

async fn run_sweep(config: Arc<Config>) -> anyhow::Result<()> {
    let db = connect_database(&config).await?;
    let cache = connect_cache(&config).await?;

    let jwt_secret = JwtSecret::try_from(config.jwt_secret.as_str())?;
    let services = Services::from_config(
        jwt_secret,
        Duration::from_secs(config.internal_jwt_ttl),
        &config.services,
        cache,
    );

    sweep_deleted_users(&db, &services, &config).await
}

async fn connect_database(config: &Config) -> anyhow::Result<DatabaseConnection> {
    info!("Connecting to database");
    let mut db_options = ConnectOptions::new(config.database.url.to_string());
    db_options.connect_timeout(Duration::from_secs(config.database.connect_timeout));
    Ok(Database::connect(db_options).await?)
}

async fn connect_cache(config: &Config) -> anyhow::Result<Cache> {
    info!("Connecting to redis");
    Ok(Cache::new(
        AsyncRedisBackend::new(
            RedisConnection::new(config.redis.challenges.as_str()).await?,
            "challenges".into(),
        ),
        PostcardFormatter,
        Duration::from_secs(config.cache_ttl),
    ))
}
