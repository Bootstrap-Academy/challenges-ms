#![forbid(unsafe_code)]
#![warn(clippy::dbg_macro, clippy::use_debug)]

use sea_orm::DatabaseConnection;
use services::Services;

use crate::jwt::JwtSecret;
use crate::redis::RedisConnection;

pub mod auth;
pub mod config;
pub mod jwt;
pub mod redis;
pub mod services;

#[derive(Debug, Clone)]
pub struct SharedState {
    pub db: DatabaseConnection,
    pub jwt_secret: JwtSecret,
    pub auth_redis: RedisConnection,
    pub services: Services,
}
