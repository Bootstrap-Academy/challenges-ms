#![forbid(unsafe_code)]
#![warn(clippy::dbg_macro, clippy::use_debug, clippy::todo)]

use fnct::{backend::AsyncRedisBackend, format::PostcardFormatter, AsyncCache};
use sea_orm::DatabaseConnection;
use services::Services;

use crate::{jwt::JwtSecret, redis::RedisConnection};

pub mod auth;
pub mod config;
pub mod jwt;
pub mod redis;
pub mod services;

pub type Cache<S = PostcardFormatter> = AsyncCache<AsyncRedisBackend<RedisConnection>, S>;
pub type CacheError<S = PostcardFormatter> = fnct::Error<AsyncRedisBackend<RedisConnection>, S>;

#[derive(Debug, Clone)]
pub struct SharedState {
    pub jwt_secret: JwtSecret,
    pub auth_redis: RedisConnection,
    pub services: Services,
    pub cache: Cache,
    pub db: DatabaseConnection,
}
