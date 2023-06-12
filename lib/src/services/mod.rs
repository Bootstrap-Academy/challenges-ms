use std::{sync::Arc, time::Duration};

use reqwest::{Client, Method, RequestBuilder, StatusCode};
use thiserror::Error;
use url::Url;

use self::{shop::ShopService, skills::SkillsService};
use crate::{
    jwt::{sign_jwt, InternalAuthToken, JwtSecret},
    Cache, CacheError,
};

pub mod shop;
pub mod skills;

#[derive(Debug, Clone)]
pub struct Services {
    pub skills: SkillsService,
    pub shop: ShopService,
}

impl Services {
    pub fn from_config(
        jwt_secret: JwtSecret,
        jwt_ttl: Duration,
        conf: &crate::config::Services,
        cache: Cache,
    ) -> Self {
        let jwt_config = Arc::new(JwtConfig {
            secret: jwt_secret,
            ttl: jwt_ttl,
        });
        Self {
            skills: SkillsService::new(Service::new(
                "skills",
                conf.skills.clone(),
                Arc::clone(&jwt_config),
                cache.clone(),
            )),
            shop: ShopService::new(Service::new("shop", conf.shop.clone(), jwt_config, cache)),
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
}

impl Service {
    fn new(name: &'static str, base_url: Url, jwt_config: Arc<JwtConfig>, cache: Cache) -> Self {
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
    CacheError(#[from] CacheError),
    #[error("unexpected response status code: {0}")]
    UnexpectedStatusCode(StatusCode),
}

pub type ServiceResult<T> = Result<T, ServiceError>;
