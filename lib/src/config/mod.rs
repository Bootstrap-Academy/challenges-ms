use std::env;

use config::{ConfigError, Environment, File};
use serde::{de::DeserializeOwned, Deserialize};
use url::Url;

use self::challenges::ChallengesConfig;

mod challenges;

pub fn load() -> Result<Config, ConfigError> {
    load_config()
}

pub fn load_database_config() -> Result<Database, ConfigError> {
    Ok(load_config::<DatabaseConfig>()?.database)
}

pub fn load_config<T: DeserializeOwned>() -> Result<T, ConfigError> {
    let path = env::var("CONFIG_PATH").unwrap_or("config.toml".to_owned());
    config::Config::builder()
        .add_source(File::with_name(&path))
        .add_source(Environment::default().separator("__"))
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
    pub challenges: ChallengesConfig,
}

#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    pub database: Database,
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

#[derive(Debug, Deserialize)]
pub struct Sentry {
    pub dsn: Url,
}
