#![forbid(unsafe_code)]
#![warn(clippy::dbg_macro, clippy::use_debug)]

use anyhow::Context;
use lib::config;
use poem::{
    listener::TcpListener,
    middleware::{CatchPanic, Tracing},
    EndpointExt, Route, Server,
};
use poem_openapi::OpenApiService;
use sea_orm::Database;
use tracing::info;

use crate::endpoints::get_api;

mod endpoints;
mod schemas;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config = config::load().context("loading config")?;

    let db = Database::connect(&config.database_url).await?;

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
        .with(CatchPanic::new());

    info!("Listening on {}:{}", config.jobs.host, config.jobs.port);
    Server::new(TcpListener::bind((config.jobs.host, config.jobs.port)))
        .run(app)
        .await?;

    Ok(())
}
