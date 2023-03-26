use std::time::Duration;

use fnct::async_redis::AsyncRedisCache;
use reqwest::{Client, Method, RequestBuilder};
use thiserror::Error;
use url::Url;

use crate::{
    jwt::{sign_jwt, InternalAuthToken, JwtSecret},
    redis::RedisConnection,
};

use self::skills::SkillsService;

pub mod skills;

#[derive(Debug, Clone)]
pub struct Services {
    pub skills: SkillsService,
}

impl Services {
    pub fn from_config(
        jwt_secret: JwtSecret,
        jwt_ttl: Duration,
        conf: &crate::config::Services,
        cache: AsyncRedisCache<RedisConnection>,
    ) -> Self {
        let jwt_config = JwtConfig {
            secret: jwt_secret,
            ttl: jwt_ttl,
        };
        Self {
            skills: SkillsService::new(Service::new(
                "skills",
                conf.skills.clone(),
                jwt_config,
                cache,
            )),
        }
    }
}

#[derive(Debug, Clone)]
struct JwtConfig {
    secret: JwtSecret,
    ttl: Duration,
}

#[derive(Debug, Clone)]
struct Service {
    name: &'static str,
    base_url: Url,
    jwt_config: JwtConfig,
    cache: AsyncRedisCache<RedisConnection>,
}

impl Service {
    fn new(
        name: &'static str,
        base_url: Url,
        jwt_config: JwtConfig,
        cache: AsyncRedisCache<RedisConnection>,
    ) -> Self {
        Self {
            name,
            base_url,
            jwt_config,
            cache,
        }
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let token = sign_jwt(
            InternalAuthToken {
                aud: self.name.into(),
            },
            &self.jwt_config.secret,
            self.jwt_config.ttl,
        )
        .expect("could not sign internal auth token");
        Client::new()
            .request(
                method,
                self.base_url
                    .join(&format!("_internal/{}", path.trim_start_matches('/')))
                    .expect("could not build url"),
            )
            .bearer_auth(token)
    }
}

macro_rules! methods {
    ($($method:ident),*) => {
        paste::paste! {
            $(
                #[allow(dead_code)]
                fn $method(&self, path: &str) -> RequestBuilder {
                    self.request(Method::[< $method:upper >], path)
                }
            )*
        }
    };
}

impl Service {
    methods!(get, post, put, patch, delete, head);
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("reqwest error: {0}")]
    ReqwestError(#[from] reqwest::Error),
    #[error("cache error: {0}")]
    CacheError(#[from] fnct::async_redis::AsyncRedisCacheError),
}

pub type ServiceResult<T> = Result<T, ServiceError>;
