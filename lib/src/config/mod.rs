use std::env;

use config::{ConfigError, File};
use serde::Deserialize;

use self::jobs::JobsConfig;

mod jobs;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub jobs: JobsConfig,
}

pub fn load() -> Result<Config, ConfigError> {
    config::Config::builder()
        .add_source(File::with_name(
            &env::var("CONFIG_PATH").unwrap_or("config.toml".to_owned()),
        ))
        .build()?
        .try_deserialize()
}
