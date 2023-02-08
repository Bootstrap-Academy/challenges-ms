#![forbid(unsafe_code)]
#![warn(clippy::dbg_macro, clippy::use_debug)]

use sea_orm_migration::prelude::*;

#[tokio::main]
async fn main() {
    cli::run_cli(migration::Migrator).await;
}
