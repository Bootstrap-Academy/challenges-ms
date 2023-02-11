use jwt::VerifyWithKey;
use poem::{http::StatusCode, Request};
use poem_openapi::auth::Bearer;

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
pub struct AdminAuth(pub User);

async fn public_auth_check(
    req: &Request,
    token: Option<Bearer>,
) -> Result<Option<User>, StatusCode> {
    Ok((|| {
        let Bearer { token } = token?;
        let jwt_secret = req
            .data::<JwtSecret>()
            .expect("request does not have JwtSecret data");
        let user = VerifyWithKey::<UserAccessToken>::verify_with_key(token.as_str(), &jwt_secret.0)
            .ok()?;
        // TODO: check token blacklist (redis)
        Some(User {
            id: user.uid,
            email_verified: user.data.email_verified,
            admin: user.data.admin,
        })
    })())
}

async fn user_auth_check(req: &Request, token: Option<Bearer>) -> Result<User, StatusCode> {
    public_auth_check(req, token)
        .await?
        .ok_or(StatusCode::UNAUTHORIZED)
}

async fn admin_auth_check(req: &Request, token: Option<Bearer>) -> Result<User, StatusCode> {
    let user = user_auth_check(req, token).await?;
    match user.admin {
        true => Ok(user),
        false => Err(StatusCode::FORBIDDEN),
    }
}

macro_rules! impl_api_extractor {
    ($auth:ident, $checker:ident) => {
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
                let output = $checker(request, output).await?;
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

impl_api_extractor!(PublicAuth, public_auth_check);
impl_api_extractor!(UserAuth, user_auth_check);
impl_api_extractor!(AdminAuth, admin_auth_check);
