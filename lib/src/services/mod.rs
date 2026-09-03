use std::{sync::Arc, time::Duration};

use fnct::format::JsonFormatter;
use reqwest::{Client, Method, RequestBuilder, StatusCode};
use thiserror::Error;
use url::Url;

use self::{auth::AuthService, shop::ShopService, skills::SkillsService};
use crate::{
    jwt::{sign_jwt, InternalAuthToken, InternalJwtSecrets, JwtSecret},
    Cache, CacheError,
};

pub mod auth;
pub mod shop;
pub mod skills;

#[derive(Debug, Clone)]
pub struct Services {
    pub auth: AuthService,
    pub skills: SkillsService,
    pub shop: ShopService,
}

impl Services {
    pub fn from_config(
        internal_jwt_secrets: &InternalJwtSecrets,
        jwt_ttl: Duration,
        conf: &crate::config::Services,
        cache: Cache,
    ) -> Self {
        // every service is addressed with the secret that belongs to its own
        // audience, so the key of one service cannot be used to talk to another
        let jwt_config = |audience: &str| {
            Arc::new(JwtConfig {
                secret: internal_jwt_secrets.get(audience).clone(),
                ttl: jwt_ttl,
            })
        };
        Self {
            auth: AuthService::new(Service::new(
                "auth",
                conf.auth.clone(),
                jwt_config("auth"),
                cache.clone(),
            )),
            skills: SkillsService::new(Service::new(
                "skills",
                conf.skills.clone(),
                jwt_config("skills"),
                cache.clone(),
            )),
            shop: ShopService::new(Service::new(
                "shop",
                conf.shop.clone(),
                jwt_config("shop"),
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
    jwt_config: Arc<JwtConfig>,
    cache: Cache,
    json_cache: Cache<JsonFormatter>,
}

impl Service {
    fn new(name: &'static str, base_url: Url, jwt_config: Arc<JwtConfig>, cache: Cache) -> Self {
        Self {
            name,
            base_url,
            jwt_config,
            json_cache: cache.with_formatter(JsonFormatter),
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
    CacheError(#[from] CacheError),
    #[error("cache error: {0}")]
    JsonCacheError(#[from] CacheError<JsonFormatter>),
    #[error("unexpected response status code: {0}")]
    UnexpectedStatusCode(StatusCode),
}

pub type ServiceResult<T> = Result<T, ServiceError>;
