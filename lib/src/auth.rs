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

/// Only explicit learning routes consume this principal. It is never an
/// ordinary session and carries no purchase, publication or administrator power.
pub struct LearningPrincipal {
    pub user: User,
    pub digest: String,
}

pub struct LearningAuth(pub LearningPrincipal);

async fn learning_auth_check(
    req: &Request,
    _: Option<Bearer>,
) -> Result<LearningPrincipal, UserAuthError::raw::Response> {
    use sha2::{Digest, Sha256};
    let key = req
        .headers()
        .get("x-learning-key")
        .and_then(|v| v.to_str().ok())
        .filter(|v| (43..=256).contains(&v.len()))
        .ok_or_else(UserAuthError::raw::unauthorized)?;
    let digest = format!("{:x}", Sha256::digest(key.as_bytes()));
    let state = req.data::<Arc<SharedState>>().expect("request SharedState");
    let user = state
        .services
        .shop
        .learning_authority(&digest)
        .await
        .map_err(|_| UserAuthError::raw::unavailable())?
        .ok_or_else(UserAuthError::raw::unauthorized)?;
    Ok(LearningPrincipal { user, digest })
}

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
        .map_err(|_| UserAuthError::raw::unavailable())?
    {
        return Err(UserAuthError::raw::unauthorized());
    }
    let authority = data
        .services
        .auth
        .ordinary_authority(&token)
        .await
        .map_err(|_| UserAuthError::raw::unavailable())?
        .filter(|a| a.id == user.uid)
        .ok_or_else(UserAuthError::raw::unauthorized)?;
    Ok(User {
        id: authority.id,
        email_verified: authority.email_verified,
        admin: authority.admin,
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
    match verify_internal_token(&token, data.internal_jwt_secrets.get(INTERNAL_AUDIENCE)) {
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
        Err(UserAuthError::raw::Response::Unauthorized(_)) => Ok(None),
        Err(error) => Err(error),
    }
});
add_response_schemas!(PublicAuth);

custom_auth!(UserAuth, user_auth_check);
add_response_schemas!(UserAuth, UserAuthError::raw::Response);

custom_auth!(VerifiedUserAuth, verified_user_auth_check);
add_response_schemas!(VerifiedUserAuth, VerifiedUserAuthError::raw::Response);

// The API description exposes the same dedicated header that the extractor
// actually consumes. No ordinary bearer scheme is advertised for this route.
impl<'a> poem_openapi::ApiExtractor<'a> for LearningAuth {
    const TYPES: &'static [poem_openapi::ApiExtractorType] =
        &[poem_openapi::ApiExtractorType::SecurityScheme];
    type ParamType = ();
    type ParamRawType = ();
    async fn from_request(
        request: &'a poem::Request,
        _body: &mut poem::RequestBody,
        _options: poem_openapi::ExtractParamOptions<Self::ParamType>,
    ) -> poem::Result<Self> {
        Ok(Self(learning_auth_check(request, None).await?))
    }
    fn register(registry: &mut poem_openapi::registry::Registry) {
        registry.create_security_scheme(
            "LearningAuth",
            poem_openapi::registry::MetaSecurityScheme {
                ty: "apiKey",
                description: Some("Scoped retained learning credential"),
                name: Some("x-learning-key"),
                key_in: Some("header"),
                scheme: None,
                bearer_format: None,
                flows: None,
                openid_connect_url: None,
            },
        );
    }
    fn security_schemes() -> Vec<&'static str> {
        vec!["LearningAuth"]
    }
}
add_response_schemas!(LearningAuth, UserAuthError::raw::Response);

custom_auth!(AdminAuth, admin_auth_check);
add_response_schemas!(AdminAuth, AdminAuthError::raw::Response);

custom_auth!(InternalAuth, internal_auth_check);
add_response_schemas!(InternalAuth, InternalAuthError::raw::Response);

response!(UserAuthError = {
    /// The user is unauthenticated.
    Unauthorized(401, error),
    /// The authoritative account check is unavailable. No ordinary authority is granted.
    Unavailable(503, error),
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
