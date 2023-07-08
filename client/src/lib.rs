#![forbid(unsafe_code)]
#![warn(clippy::dbg_macro, clippy::use_debug, clippy::todo)]

use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;

pub mod challenges;

#[derive(Debug, thiserror::Error)]
pub enum Error<E = ()> {
    /// The endpoint url could not be parsed.
    #[error("could not parse url: {0}")]
    UrlParseError(#[from] url::ParseError),
    /// [`reqwest`] returned an error.
    #[error("reqwest error: {0}")]
    ReqwestError(#[from] reqwest::Error),
    /// An error response that any endpoint may return.
    #[error("common error: {0:?}")]
    CommonError(CommonError),
    /// An error response that a specific endpoint may return.
    #[error("endpoint error: {0:?}")]
    EndpointError(E),
    /// An unknown error.
    #[error("unknown error: {0:?}")]
    UnknownError(Value),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "error", content = "reason", rename_all = "snake_case")]
pub enum CommonError {
    /// 422 Unprocessable Content
    UnprocessableContent(String),
    /// 500 Internal Server Error
    InternalServerError,

    /// 401 Unauthorized
    Unauthorized,
    /// 403 User is not verified
    Unverified,
    /// 403 Forbidden
    Forbidden,
}

pub(crate) fn parse_error<E: DeserializeOwned>(value: Value) -> Error<E> {
    if let Ok(x) = serde_json::from_value(value.clone()) {
        Error::CommonError(x)
    } else if let Ok(x) = serde_json::from_value(value.clone()) {
        Error::EndpointError(x)
    } else {
        Error::UnknownError(value)
    }
}

macro_rules! client {
    (
        $name:ident {
            $(
                $(#[doc = $doc:literal])*
                $vis:vis
                $func:ident($(path: $args:ident),* $(,)? $(json: $data:ty)?):
                $method:ident
                $path:literal =>
                $ok:ty
                $(, $err:ty)?;
            )*
        }
    ) => {
        use crate::Success;

        paste::paste! {
            #[derive(Debug, Clone)]
            pub struct [< $name:camel Client >] {
                base_url: url::Url,
                client: reqwest::Client,
                token: Option<String>,
            }

            #[derive(Debug, Clone)]
            pub struct [< Blocking $name:camel Client >] {
                base_url: url::Url,
                client: reqwest::blocking::Client,
                token: Option<String>,
            }

            impl [< $name:camel Client >] {

                pub fn new(base_url: url::Url, token: Option<String>) -> Self {
                    Self {
                        base_url,
                        client: reqwest::Client::new(),
                        token,
                    }
                }

                pub fn update_token(&mut self, token: Option<String>) {
                    self.token = token;
                }

                $(
                    $(#[doc = $doc])*
                    $vis async fn $func(&self, $($args: impl std::fmt::Display,)* $(data: &$data)?) -> Result<$ok, crate::Error<$($err)?>> {
                        let mut request = self
                            .client
                            .$method(self.base_url.join(&format!($path))?);
                        if let Some(token) = &self.token {
                            request = request.bearer_auth(token);
                        }
                        let response = request
                            $(.json(data as &$data))?
                            .send()
                            .await?;
                        if response.status().is_success() {
                            Ok(response.json().await?)
                        } else {
                            Err(crate::parse_error(response.json().await?))
                        }
                    }
                )*
            }

            impl [< Blocking $name:camel Client >] {
                pub fn new(base_url: url::Url, token: Option<String>) -> Self {
                    Self {
                        base_url,
                        client: reqwest::blocking::Client::new(),
                        token,
                    }
                }

                pub fn update_token(&mut self, token: Option<String>) {
                    self.token = token;
                }

                $(
                    $(#[doc = $doc])*
                    $vis fn $func(&self, $($args: impl std::fmt::Display,)* $(data: &$data)?) -> Result<$ok, crate::Error<$($err)?>> {
                        let mut request = self
                            .client
                            .$method(self.base_url.join(&format!($path))?);
                        if let Some(token) = &self.token {
                            request = request.bearer_auth(token);
                        }
                        let response = request
                            $(.json(data as &$data))?
                            .send()?;
                        if response.status().is_success() {
                            Ok(response.json()?)
                        } else {
                            Err(crate::parse_error(response.json()?))
                        }
                    }
                )*
            }
        }
    };
}

pub(crate) use client;

#[derive(Debug, Deserialize)]
pub struct Success {}
