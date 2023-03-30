use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct JobsConfig {
    pub host: String,
    pub port: u16,
    pub server: String,
}
