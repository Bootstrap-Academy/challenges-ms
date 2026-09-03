use std::{
    borrow::Cow,
    collections::HashMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use hmac::{digest::InvalidLength, Hmac, Mac};
use jwt::{SignWithKey, VerifyWithKey};
use redis::{AsyncCommands, RedisResult};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::Sha256;
use thiserror::Error;
use uuid::Uuid;

use crate::redis::RedisConnection;

#[derive(Debug, Clone)]
pub struct JwtSecret(pub Hmac<Sha256>);

impl TryFrom<&str> for JwtSecret {
    type Error = InvalidLength;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Ok(Self(Hmac::<Sha256>::new_from_slice(value.as_bytes())?))
    }
}

/// The secrets used to sign and verify internal auth tokens, one per audience.
///
/// An audience without its own secret falls back to the shared jwt secret, so a
/// deployment which has not rolled out the per audience secrets yet keeps
/// working.
#[derive(Debug, Clone)]
pub struct InternalJwtSecrets {
    fallback: JwtSecret,
    per_audience: HashMap<String, JwtSecret>,
}

impl InternalJwtSecrets {
    pub fn new(
        fallback: JwtSecret,
        secrets: &HashMap<String, String>,
    ) -> Result<Self, InvalidLength> {
        Ok(Self {
            fallback,
            per_audience: secrets
                .iter()
                .filter(|(_, secret)| !secret.is_empty())
                .map(|(audience, secret)| {
                    Ok((audience.clone(), JwtSecret::try_from(secret.as_str())?))
                })
                .collect::<Result<_, InvalidLength>>()?,
        })
    }

    /// Return the secret internal auth tokens for the given audience are signed
    /// and verified with.
    pub fn get(&self, audience: &str) -> &JwtSecret {
        self.per_audience.get(audience).unwrap_or(&self.fallback)
    }
}

#[derive(Serialize, Deserialize)]
pub struct UserAccessToken {
    pub uid: Uuid,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sign_with(secret: &JwtSecret) -> String {
        sign_jwt(
            InternalAuthToken { aud: "auth".into() },
            secret,
            Duration::from_secs(10),
        )
        .unwrap()
    }

    #[test]
    fn internal_jwt_secrets_fall_back_to_the_shared_secret() {
        let fallback = JwtSecret::try_from("shared secret").unwrap();
        let secrets = InternalJwtSecrets::new(fallback.clone(), &HashMap::new()).unwrap();

        let token = sign_with(&fallback);

        assert!(verify_jwt::<InternalAuthToken>(&token, secrets.get("auth")).is_ok());
    }

    #[test]
    fn internal_jwt_secrets_ignore_empty_values() {
        let fallback = JwtSecret::try_from("shared secret").unwrap();
        let secrets = InternalJwtSecrets::new(
            fallback.clone(),
            &HashMap::from([("auth".to_owned(), String::new())]),
        )
        .unwrap();

        let token = sign_with(&fallback);

        assert!(verify_jwt::<InternalAuthToken>(&token, secrets.get("auth")).is_ok());
    }

    #[test]
    fn internal_jwt_secrets_separate_the_audiences() {
        let fallback = JwtSecret::try_from("shared secret").unwrap();
        let secrets = InternalJwtSecrets::new(
            fallback.clone(),
            &HashMap::from([
                ("auth".to_owned(), "auth secret".to_owned()),
                ("skills".to_owned(), "skills secret".to_owned()),
            ]),
        )
        .unwrap();

        // a token signed with the shared secret is no longer accepted for an
        // audience which has its own secret
        assert!(
            verify_jwt::<InternalAuthToken>(&sign_with(&fallback), secrets.get("auth")).is_err()
        );
        assert!(verify_jwt::<InternalAuthToken>(
            &sign_with(secrets.get("skills")),
            secrets.get("auth")
        )
        .is_err());
        assert!(verify_jwt::<InternalAuthToken>(
            &sign_with(secrets.get("auth")),
            secrets.get("auth")
        )
        .is_ok());
        // an audience without its own secret still uses the shared one
        assert!(
            verify_jwt::<InternalAuthToken>(&sign_with(&fallback), secrets.get("shop")).is_ok()
        );
    }
}
