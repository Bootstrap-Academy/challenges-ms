use jwt::VerifyWithKey;
use poem::Request;
use poem_ext::{add_response_schemas, custom_auth, response};
use poem_openapi::auth::Bearer;

use crate::{jwt::UserAccessToken, SharedState};

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

async fn user_auth_check(
    req: &Request,
    token: Option<Bearer>,
) -> Result<User, UserAuthError::raw::Response> {
    let Bearer { token } = token.ok_or_else(UserAuthError::raw::unauthorized)?;
    let data = req
        .data::<SharedState>()
        .expect("request does not have a SharedState");
    let user =
        VerifyWithKey::<UserAccessToken>::verify_with_key(token.as_str(), &data.jwt_secret.0)
            .map_err(|_| UserAuthError::raw::unauthorized())?;
    if user
        .is_revoked(&data.auth_redis)
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
    match user.email_verified {
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
