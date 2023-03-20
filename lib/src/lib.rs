#![forbid(unsafe_code)]
#![warn(clippy::dbg_macro, clippy::use_debug)]

use crate::jwt::JwtSecret;

pub mod auth;
pub mod config;
pub mod jwt;

#[derive(Debug, Clone)]
pub struct SharedState {
    pub jwt_secret: JwtSecret,
    pub auth_redis: redis::Client,
}
