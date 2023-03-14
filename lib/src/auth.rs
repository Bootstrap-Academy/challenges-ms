use jwt::VerifyWithKey;
use poem::Request;
use poem_ext::{add_response_schemas, custom_auth};
use poem_openapi::{auth::Bearer, payload::Json, ApiResponse, Object};

use crate::jwt::{JwtSecret, UserAccessToken};

#[derive(Debug)]
pub struct User {
    pub id: String,
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

async fn user_auth_check(req: &Request, token: Option<Bearer>) -> Result<User, UserAuthResponse> {
    let Bearer { token } =
        token.ok_or_else(|| unauthorized("No bearer token in Authorization header"))?;
    let jwt_secret = req
        .data::<JwtSecret>()
        .expect("request does not have JwtSecret data");
    let user = VerifyWithKey::<UserAccessToken>::verify_with_key(token.as_str(), &jwt_secret.0)
        .map_err(|_| unauthorized("Invalid bearer token"))?;
    // TODO: check token blacklist (redis)
    Ok(User {
        id: user.uid,
        email_verified: user.data.email_verified,
        admin: user.data.admin,
    })
}

async fn verified_user_auth_check(
    req: &Request,
    token: Option<Bearer>,
) -> Result<User, VerifiedUserAuthResponse> {
    let user = user_auth_check(req, token).await?;
    match user.email_verified {
        true => Ok(user),
        false => Err(not_verified("Unverified user email")),
    }
}

async fn admin_auth_check(req: &Request, token: Option<Bearer>) -> Result<User, AdminAuthResponse> {
    let user = user_auth_check(req, token).await?;
    match user.admin {
        true => Ok(user),
        false => Err(forbidden("User is not an administrator")),
    }
}

custom_auth!(PublicAuth, |req, token| async move {
    match user_auth_check(req, token).await {
        Ok(user) => Ok::<_, UserAuthResponse>(Some(user)),
        Err(_) => Ok(None),
    }
});
add_response_schemas!(PublicAuth);

custom_auth!(UserAuth, user_auth_check);
add_response_schemas!(UserAuth, UserAuthResponse);

custom_auth!(VerifiedUserAuth, verified_user_auth_check);
add_response_schemas!(VerifiedUserAuth, VerifiedUserAuthResponse);

custom_auth!(AdminAuth, admin_auth_check);
add_response_schemas!(AdminAuth, AdminAuthResponse);

#[derive(Object)]
struct Error {
    error: String,
    reason: String,
}

#[derive(ApiResponse)]
enum UserAuthResponse {
    /// The user is unauthenticated.
    #[oai(status = 401)]
    Unauthorized(Json<Error>),
}

#[derive(ApiResponse)]
enum VerifiedUserAuthResponse {
    /// The user is unauthenticated.
    #[oai(status = 401)]
    Unauthorized(Json<Error>),
    /// The user is not verified
    #[oai(status = 403)]
    NotVerified(Json<Error>),
}

#[derive(ApiResponse)]
enum AdminAuthResponse {
    /// The user is unauthenticated.
    #[oai(status = 401)]
    Unauthorized(Json<Error>),
    /// The authenticated user is not allowed to perform this action.
    #[oai(status = 403)]
    Forbidden(Json<Error>),
}

impl From<UserAuthResponse> for VerifiedUserAuthResponse {
    fn from(value: UserAuthResponse) -> Self {
        match value {
            UserAuthResponse::Unauthorized(data) => Self::Unauthorized(data),
        }
    }
}

impl From<UserAuthResponse> for AdminAuthResponse {
    fn from(value: UserAuthResponse) -> Self {
        match value {
            UserAuthResponse::Unauthorized(data) => Self::Unauthorized(data),
        }
    }
}

macro_rules! error {
    ($error:ident, $resp:path, $var:ident) => {
        fn $error(reason: impl Into<String>) -> $resp {
            <$resp>::$var(Json(Error {
                error: stringify!($error).into(),
                reason: reason.into(),
            }))
        }
    };
}

error!(unauthorized, UserAuthResponse, Unauthorized);
error!(not_verified, VerifiedUserAuthResponse, NotVerified);
error!(forbidden, AdminAuthResponse, Forbidden);
