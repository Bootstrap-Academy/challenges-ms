use anyhow::Context;
use poem::{
    listener::TcpListener,
    middleware::{CatchPanic, Tracing},
    EndpointExt, Route, Server,
};
use poem_openapi::OpenApiService;
use sea_orm::Database;
use tracing::info;

use crate::endpoints::get_endpoints;

mod config;
mod endpoints;
mod schemas;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config = config::load_config().context("loading config")?;

    let db = Database::connect(&config.database_url).await?;

    let api_service = OpenApiService::new(
        get_endpoints(db),
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

    info!("Listening on {}:{}", config.host, config.port);
    Server::new(TcpListener::bind((config.host, config.port)))
        .run(app)
        .await?;

    Ok(())
}
