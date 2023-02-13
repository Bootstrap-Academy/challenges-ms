use jwt::VerifyWithKey;
use poem::Request;
use poem_openapi::{
    auth::Bearer,
    payload::Json,
    registry::{MetaResponse, Registry},
    ApiResponse, Object,
};

use crate::{
    jwt::{JwtSecret, UserAccessToken},
    types::MetaResponsesExt,
};

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

async fn user_auth_check(req: &Request, token: Option<Bearer>) -> Result<User, Response> {
    let Bearer { token } =
        token.ok_or_else(|| Response::unauthorized("No bearer token in Authorization header"))?;
    let jwt_secret = req
        .data::<JwtSecret>()
        .expect("request does not have JwtSecret data");
    let user = VerifyWithKey::<UserAccessToken>::verify_with_key(token.as_str(), &jwt_secret.0)
        .map_err(|_| Response::unauthorized("Invalid bearer token"))?;
    // TODO: check token blacklist (redis)
    Ok(User {
        id: user.uid,
        email_verified: user.data.email_verified,
        admin: user.data.admin,
    })
}

async fn verified_user_auth_check(req: &Request, token: Option<Bearer>) -> Result<User, Response> {
    let user = user_auth_check(req, token).await?;
    match user.email_verified {
        true => Ok(user),
        false => Err(Response::forbidden("Unverified user email")),
    }
}

async fn admin_auth_check(req: &Request, token: Option<Bearer>) -> Result<User, Response> {
    let user = user_auth_check(req, token).await?;
    match user.admin {
        true => Ok(user),
        false => Err(Response::forbidden("User is not an administrator")),
    }
}

impl MetaResponsesExt for PublicAuth {
    type Iter = Vec<MetaResponse>;
    fn responses() -> Self::Iter {
        vec![]
    }
    fn register(_registry: &mut Registry) {}
}

impl MetaResponsesExt for UserAuth {
    type Iter = Vec<MetaResponse>;
    fn responses() -> Self::Iter {
        Response::meta()
            .responses
            .into_iter()
            .filter(|x| matches!(x.status, Some(401)))
            .collect()
    }
    fn register(registry: &mut Registry) {
        Response::register(registry);
    }
}

impl MetaResponsesExt for VerifiedUserAuth {
    type Iter = Vec<MetaResponse>;
    fn responses() -> Self::Iter {
        Response::meta().responses
    }
    fn register(registry: &mut Registry) {
        Response::register(registry);
    }
}

impl MetaResponsesExt for AdminAuth {
    type Iter = Vec<MetaResponse>;
    fn responses() -> Self::Iter {
        Response::meta().responses
    }
    fn register(registry: &mut Registry) {
        Response::register(registry);
    }
}

macro_rules! impl_api_extractor {
    ($auth:ident, $checker:expr) => {
        #[poem::async_trait]
        impl<'a> poem_openapi::ApiExtractor<'a> for $auth {
            const TYPE: poem_openapi::ApiExtractorType =
                poem_openapi::ApiExtractorType::SecurityScheme;

            type ParamType = ();
            type ParamRawType = ();

            async fn from_request(
                request: &'a Request,
                _body: &mut poem::RequestBody,
                _param_opts: poem_openapi::ExtractParamOptions<Self::ParamType>,
            ) -> poem::Result<Self> {
                let output =
                    <Bearer as poem_openapi::auth::BearerAuthorization>::from_request(request).ok();
                let checker = $checker;
                let output = checker(request, output).await?;
                Ok(Self(output))
            }

            fn register(registry: &mut poem_openapi::registry::Registry) {
                registry.create_security_scheme(
                    stringify!($auth),
                    poem_openapi::registry::MetaSecurityScheme {
                        ty: "http",
                        description: None,
                        name: None,
                        key_in: None,
                        scheme: Some("bearer"),
                        bearer_format: None,
                        flows: None,
                        openid_connect_url: None,
                    },
                );
            }

            fn security_scheme() -> Option<&'static str> {
                Some(stringify!($auth))
            }
        }
    };
}

impl_api_extractor!(PublicAuth, |req, token| async move {
    match user_auth_check(req, token).await {
        Ok(user) => Ok::<_, Response>(Some(user)),
        Err(_) => Ok(None),
    }
});
impl_api_extractor!(UserAuth, user_auth_check);
impl_api_extractor!(VerifiedUserAuth, verified_user_auth_check);
impl_api_extractor!(AdminAuth, admin_auth_check);

#[derive(Object)]
struct Error {
    error: String,
    reason: String,
}

#[derive(ApiResponse)]
enum Response {
    /// The user is unauthenticated.
    #[oai(status = 401)]
    Unauthorized(Json<Error>),
    /// The authenticated user is not allowed to perform this action.
    #[oai(status = 403)]
    Forbidden(Json<Error>),
}

impl Response {
    fn unauthorized(reason: impl Into<String>) -> Self {
        Self::Unauthorized(Json(Error {
            error: "unauthorized".into(),
            reason: reason.into(),
        }))
    }

    fn forbidden(reason: impl Into<String>) -> Self {
        Self::Forbidden(Json(Error {
            error: "forbidden".into(),
            reason: reason.into(),
        }))
    }
}
