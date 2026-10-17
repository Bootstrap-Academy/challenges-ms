use std::{collections::HashMap, env};

use config::{ConfigError, Environment, File};
use serde::{de::DeserializeOwned, Deserialize};
use url::Url;

use self::challenges::ChallengesConfig;
pub use self::challenges::CodingExecution;

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
    /// Secrets for the internal auth tokens, one per audience. An audience
    /// which is missing here falls back to [`Config::jwt_secret`].
    #[serde(default)]
    pub internal_jwt_secrets: HashMap<String, String>,
    pub internal_jwt_ttl: u64,
    pub cache_ttl: u64,
    pub database: Database,
    pub redis: Redis,
    pub services: Services,
    pub challenges: ChallengesConfig,
    #[serde(default)]
    pub deleted_user_sweep: DeletedUserSweep,
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

#[derive(Debug, Deserialize)]
pub struct DeletedUserSweep {
    #[serde(default = "default_batch_size")]
    pub batch_size: u64,
    /// Maximum number of auth microservice requests per second (0 = unlimited).
    #[serde(default = "default_rate_limit")]
    pub rate_limit: u32,
    /// Number of missing users a run always accepts.
    ///
    /// If more users are missing than this and than [`Self::max_missing_share`]
    /// of the checked users, the run deletes nothing and fails, because a broken
    /// auth microservice is then more likely than that many deleted accounts.
    #[serde(default = "default_max_missing")]
    pub max_missing: u64,
    /// Share of the checked users (between 0 and 1) that may be missing in one
    /// run, see [`Self::max_missing`].
    #[serde(default = "default_max_missing_share")]
    pub max_missing_share: f64,
}

impl Default for DeletedUserSweep {
    fn default() -> Self {
        Self {
            batch_size: default_batch_size(),
            rate_limit: default_rate_limit(),
            max_missing: default_max_missing(),
            max_missing_share: default_max_missing_share(),
        }
    }
}

fn default_batch_size() -> u64 {
    500
}

fn default_rate_limit() -> u32 {
    10
}

fn default_max_missing() -> u64 {
    25
}

fn default_max_missing_share() -> f64 {
    0.01
}

#[cfg(test)]
mod tests {
    use config::FileFormat;

    use super::*;

    #[test]
    fn test_deleted_user_sweep_defaults() {
        let deleted_user_sweep: DeletedUserSweep = config::Config::builder()
            .add_source(File::from_str("", FileFormat::Toml))
            .build()
            .unwrap()
            .try_deserialize()
            .unwrap();
        assert_eq!(deleted_user_sweep.batch_size, 500);
        assert_eq!(deleted_user_sweep.rate_limit, 10);
        assert_eq!(deleted_user_sweep.max_missing, 25);
        assert_eq!(deleted_user_sweep.max_missing_share, 0.01);
    }
}
