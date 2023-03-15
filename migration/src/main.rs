#![forbid(unsafe_code)]
#![warn(clippy::dbg_macro, clippy::use_debug)]

use std::env;

use sea_orm_migration::prelude::*;

#[tokio::main]
async fn main() {
    if env::var("DATABASE_URL").is_err() {
        let config = lib::config::load().unwrap();
        env::set_var("DATABASE_URL", config.database.url);
    }
    cli::run_cli(migration::Migrator).await;
}
