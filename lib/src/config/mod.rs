use std::env;

use config::{ConfigError, File};
use serde::Deserialize;
use url::Url;

use self::{challenges::ChallengesConfig, jobs::JobsConfig};

mod challenges;
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
    pub jwt_secret: String,
    pub internal_jwt_ttl: u64,
    pub cache_ttl: u64,
    pub database: Database,
    pub redis: Redis,
    pub services: Services,
    pub jobs: JobsConfig,
    pub challenges: ChallengesConfig,
}

#[derive(Debug, Deserialize)]
pub struct Database {
    pub url: Url,
    pub connect_timeout: u64,
}

#[derive(Debug, Deserialize)]
pub struct Redis {
    pub auth: Url,
    pub skills: Url,
    pub shop: Url,
    pub jobs: Url,
    pub events: Url,
    pub challenges: Url,
}

#[derive(Debug, Deserialize)]
pub struct Services {
    pub auth: Url,
    pub skills: Url,
    pub shop: Url,
    pub jobs: Url,
    pub events: Url,
    pub challenges: Url,
}
