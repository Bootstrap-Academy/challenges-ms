use std::env;

use config::{ConfigError, File};
use serde::Deserialize;

use self::jobs::JobsConfig;

mod jobs;

pub fn load() -> Result<Config, ConfigError> {
    config::Config::builder()
        .add_source(File::with_name(
            &env::var("CONFIG_PATH").unwrap_or("config.toml".to_owned()),
        ))
        .build()?
        .try_deserialize()
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub database: Database,
    pub jwt_secret: String,
    pub jobs: JobsConfig,
}

#[derive(Debug, Deserialize)]
pub struct Database {
    pub url: String,
    pub connect_timeout: u64,
}
