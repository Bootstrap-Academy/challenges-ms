#![forbid(unsafe_code)]
#![warn(clippy::dbg_macro, clippy::use_debug)]

use std::time::Duration;

use lib::{config, jwt::JwtSecret, SharedState};
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
    let mut db_options = ConnectOptions::new(config.database.url);
    db_options.connect_timeout(Duration::from_secs(config.database.connect_timeout));
    let db = Database::connect(db_options).await?;

    info!("Connecting to redis");
    let _redis = redis::Client::open(config.redis.jobs)?;
    let auth_redis = redis::Client::open(config.redis.auth)?;

    let api_service = OpenApiService::new(
        get_api(db),
        "Bootstrap Academy Backend: Jobs Microservice",
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
        .data(SharedState {
            jwt_secret: JwtSecret::try_from(config.jwt_secret.as_str())?,
            auth_redis,
        });

    info!("Listening on {}:{}", config.jobs.host, config.jobs.port);
    Server::new(TcpListener::bind((config.jobs.host, config.jobs.port)))
        .run(app)
        .await?;

    Ok(())
}
