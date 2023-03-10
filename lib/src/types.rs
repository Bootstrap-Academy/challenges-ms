use std::marker::PhantomData;

use poem::IntoResponse;
use poem_openapi::{
    payload::Json,
    registry::{MetaResponse, MetaResponses, Registry},
    ApiResponse, Object,
};

pub type Response<T, A = ()> = poem::Result<InnerResponse<T, A>>;

pub enum InnerResponse<T, A> {
    Ok { value: T, _auth: PhantomData<A> },
    BadRequest { error: poem::Error },
}

impl<T, A> From<T> for InnerResponse<T, A> {
    fn from(value: T) -> Self {
        Self::Ok {
            value,
            _auth: PhantomData,
        }
    }
}

pub trait MetaResponsesExt {
    type Iter: IntoIterator<Item = MetaResponse>;
    fn responses() -> Self::Iter;
    fn register(registry: &mut Registry);
}

impl MetaResponsesExt for () {
    type Iter = Vec<MetaResponse>;
    fn responses() -> Self::Iter {
        vec![]
    }
    fn register(_registry: &mut Registry) {}
}

#[derive(Object)]
struct BadRequestError {
    reason: String,
}

#[derive(ApiResponse)]
enum BadRequestResponse {
    /// Unprocessable Content
    #[oai(status = 422)]
    UnprocessableContent(Json<BadRequestError>),
}

impl<T, A> ApiResponse for InnerResponse<T, A>
where
    A: MetaResponsesExt,
    T: ApiResponse,
{
    const BAD_REQUEST_HANDLER: bool = true;

    fn meta() -> MetaResponses {
        let MetaResponses { mut responses } = T::meta();
        responses.extend(A::responses());
        responses.extend(BadRequestResponse::meta().responses);
        MetaResponses { responses }
    }

    fn register(registry: &mut Registry) {
        T::register(registry);
        A::register(registry);
        BadRequestResponse::register(registry);
    }

    fn from_parse_request_error(error: poem::Error) -> Self {
        Self::BadRequest { error }
    }
}

impl<T, A> IntoResponse for InnerResponse<T, A>
where
    A: Send,
    T: IntoResponse,
{
    fn into_response(self) -> poem::Response {
        match self {
            InnerResponse::Ok { value, _auth } => value.into_response(),
            InnerResponse::BadRequest { error } => {
                BadRequestResponse::UnprocessableContent(Json(BadRequestError {
                    reason: error.to_string(),
                }))
                .into_response()
            }
        }
    }
}
