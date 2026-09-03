use std::sync::Arc;

use poem::Request;
use poem_ext::{add_response_schemas, custom_auth, response};
use poem_openapi::auth::Bearer;
use tracing::debug;
use uuid::Uuid;

use crate::{
    jwt::{verify_jwt, InternalAuthToken, JwtSecret, UserAccessToken},
    SharedState,
};

/// The audience internal auth tokens have to be issued for to be accepted by
/// this microservice.
const INTERNAL_AUDIENCE: &str = "challenges";

#[derive(Debug)]
pub struct User {
    pub id: Uuid,
    pub email_verified: bool,
    pub admin: bool,
}

#[derive(Debug)]
pub struct PublicAuth(pub Option<User>);

#[derive(Debug)]
pub struct UserAuth(pub User);

#[derive(Debug)]
pub struct VerifiedUserAuth(pub User);

#[derive(Debug)]
pub struct AdminAuth(pub User);

#[derive(Debug)]
pub struct InternalAuth(pub ());

async fn user_auth_check(
    req: &Request,
    token: Option<Bearer>,
) -> Result<User, UserAuthError::raw::Response> {
    let Bearer { token } = token.ok_or_else(UserAuthError::raw::unauthorized)?;
    let data = req
        .data::<Arc<SharedState>>()
        .expect("request does not have a SharedState");
    let user: UserAccessToken = verify_jwt(&token, &data.jwt_secret).map_err(|err| {
        debug!("jwt token verification failed: {err}");
        UserAuthError::raw::unauthorized()
    })?;
    if user
        .is_revoked(&mut data.auth_redis.clone())
        .await
        .expect("token verification via auth redis failed")
    {
        return Err(UserAuthError::raw::unauthorized());
    }
    Ok(User {
        id: user.uid,
        email_verified: user.data.email_verified,
        admin: user.data.admin,
    })
}

async fn verified_user_auth_check(
    req: &Request,
    token: Option<Bearer>,
) -> Result<User, VerifiedUserAuthError::raw::Response> {
    let user = user_auth_check(req, token).await?;
    match user.email_verified | user.admin {
        true => Ok(user),
        false => Err(VerifiedUserAuthError::raw::unverified()),
    }
}

async fn admin_auth_check(
    req: &Request,
    token: Option<Bearer>,
) -> Result<User, AdminAuthError::raw::Response> {
    let user = user_auth_check(req, token).await?;
    match user.admin {
        true => Ok(user),
        false => Err(AdminAuthError::raw::forbidden()),
    }
}

async fn internal_auth_check(
    req: &Request,
    token: Option<Bearer>,
) -> Result<(), InternalAuthError::raw::Response> {
    let Bearer { token } = token.ok_or_else(InternalAuthError::raw::unauthorized)?;
    let data = req
        .data::<Arc<SharedState>>()
        .expect("request does not have a SharedState");
    match verify_internal_token(&token, &data.jwt_secret) {
        true => Ok(()),
        false => Err(InternalAuthError::raw::unauthorized()),
    }
}

/// Check whether the given token is an internal auth token that has been issued
/// for this microservice.
fn verify_internal_token(token: &str, secret: &JwtSecret) -> bool {
    match verify_jwt::<InternalAuthToken>(token, secret) {
        Ok(token) => token.aud == INTERNAL_AUDIENCE,
        Err(err) => {
            debug!("jwt token verification failed: {err}");
            false
        }
    }
}

custom_auth!(PublicAuth, |req, token| async move {
    match user_auth_check(req, token).await {
        Ok(user) => Ok::<_, UserAuthError::raw::Response>(Some(user)),
        Err(_) => Ok(None),
    }
});
add_response_schemas!(PublicAuth);

custom_auth!(UserAuth, user_auth_check);
add_response_schemas!(UserAuth, UserAuthError::raw::Response);

custom_auth!(VerifiedUserAuth, verified_user_auth_check);
add_response_schemas!(VerifiedUserAuth, VerifiedUserAuthError::raw::Response);

custom_auth!(AdminAuth, admin_auth_check);
add_response_schemas!(AdminAuth, AdminAuthError::raw::Response);

custom_auth!(InternalAuth, internal_auth_check);
add_response_schemas!(InternalAuth, InternalAuthError::raw::Response);

response!(UserAuthError = {
    /// The user is unauthenticated.
    Unauthorized(401, error),
});

response!(VerifiedUserAuthError = {
    /// The authenticated user is not verified.
    Unverified(403, error),
    ..UserAuthError::raw::Response,
});

response!(AdminAuthError = {
    /// The authenticated user is not allowed to perform this action.
    Forbidden(403, error),
    ..UserAuthError::raw::Response,
});

response!(InternalAuthError = {
    /// The internal auth token is missing or invalid.
    Unauthorized(401, error),
});

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::jwt::sign_jwt;

    fn sign_internal_token(aud: &'static str, secret: &JwtSecret, ttl: Duration) -> String {
        sign_jwt(InternalAuthToken { aud: aud.into() }, secret, ttl).unwrap()
    }

    #[test]
    fn test_internal_token() {
        let secret = JwtSecret::try_from("secret").unwrap();
        let token = sign_internal_token(INTERNAL_AUDIENCE, &secret, Duration::from_secs(10));
        assert!(verify_internal_token(&token, &secret));
    }

    #[test]
    fn test_internal_token_wrong_audience() {
        let secret = JwtSecret::try_from("secret").unwrap();
        let token = sign_internal_token("skills", &secret, Duration::from_secs(10));
        assert!(!verify_internal_token(&token, &secret));
    }

    #[test]
    fn test_internal_token_wrong_secret() {
        let secret = JwtSecret::try_from("secret").unwrap();
        let other_secret = JwtSecret::try_from("other secret").unwrap();
        let token = sign_internal_token(INTERNAL_AUDIENCE, &other_secret, Duration::from_secs(10));
        assert!(!verify_internal_token(&token, &secret));
    }

    #[test]
    fn test_internal_token_expired() {
        let secret = JwtSecret::try_from("secret").unwrap();
        let token = sign_internal_token(INTERNAL_AUDIENCE, &secret, Duration::ZERO);
        assert!(!verify_internal_token(&token, &secret));
    }

    #[test]
    fn test_internal_token_malformed() {
        let secret = JwtSecret::try_from("secret").unwrap();
        assert!(!verify_internal_token("not a jwt", &secret));
    }
}
