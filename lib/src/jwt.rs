use std::{
    borrow::Cow,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use hmac::{digest::InvalidLength, Hmac, Mac};
use jwt::{SignWithKey, VerifyWithKey};
use redis::{AsyncCommands, RedisResult};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::Sha256;
use thiserror::Error;

use crate::redis::RedisConnection;

#[derive(Debug, Clone)]
pub struct JwtSecret(pub Hmac<Sha256>);

impl TryFrom<&str> for JwtSecret {
    type Error = InvalidLength;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Ok(Self(Hmac::<Sha256>::new_from_slice(value.as_bytes())?))
    }
}

#[derive(Serialize, Deserialize)]
pub struct UserAccessToken {
    pub uid: String,
    pub rt: String,
    pub data: UserAccessTokenData,
}

impl UserAccessToken {
    pub async fn is_revoked(&self, redis: &mut RedisConnection) -> RedisResult<bool> {
        redis.exists(format!("session_logout:{}", self.rt)).await
    }
}

#[derive(Serialize, Deserialize)]
pub struct UserAccessTokenData {
    pub email_verified: bool,
    pub admin: bool,
}

#[derive(Serialize, Deserialize)]
pub struct InternalAuthToken {
    pub aud: Cow<'static, str>,
}

pub fn sign_jwt(
    data: impl Serialize,
    secret: &JwtSecret,
    ttl: Duration,
) -> Result<String, JwtError> {
    let mut data = match serde_json::to_value(data)? {
        Value::Null => return Err(JwtError::NoObject("null")),
        Value::Bool(_) => return Err(JwtError::NoObject("bool")),
        Value::Number(_) => return Err(JwtError::NoObject("number")),
        Value::String(_) => return Err(JwtError::NoObject("string")),
        Value::Array(_) => return Err(JwtError::NoObject("array")),
        Value::Object(x) => x,
    };
    data.insert(
        "exp".into(),
        json!((SystemTime::now().duration_since(UNIX_EPOCH).unwrap() + ttl).as_secs()),
    );
    Ok(serde_json::to_value(data)
        .unwrap()
        .sign_with_key(&secret.0)?)
}

pub fn verify_jwt<T: DeserializeOwned>(jwt: &str, secret: &JwtSecret) -> Result<T, JwtError> {
    let data = VerifyWithKey::<Map<String, Value>>::verify_with_key(jwt, &secret.0)?;

    let exp = data
        .get("exp")
        .and_then(|x| x.as_u64())
        .ok_or(JwtError::NoExpiration)?;
    if exp
        <= SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    {
        return Err(JwtError::Expired(exp));
    }

    Ok(serde_json::from_value(Value::Object(data))?)
}

#[derive(Debug, Error)]
pub enum JwtError {
    #[error("jwt error: {0}")]
    JwtError(#[from] jwt::Error),
    #[error("deserialization error: {0}")]
    DeserializationError(#[from] serde_json::Error),
    #[error("token expired at t={0}")]
    Expired(u64),
    #[error("no exp field in token")]
    NoExpiration,
    #[error("can only sign objects (trying to serialize {0})")]
    NoObject(&'static str),
}
