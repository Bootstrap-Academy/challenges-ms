use hmac::{digest::InvalidLength, Hmac, Mac};
use redis::{AsyncCommands, RedisResult};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

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
    pub async fn is_revoked(&self, redis: &redis::Client) -> RedisResult<bool> {
        let mut conn = redis.get_async_connection().await?;
        conn.exists(format!("session_logout:{}", self.rt)).await
    }
}

#[derive(Serialize, Deserialize)]
pub struct UserAccessTokenData {
    pub email_verified: bool,
    pub admin: bool,
}
