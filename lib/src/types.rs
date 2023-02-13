use std::marker::PhantomData;

use poem::IntoResponse;
use poem_openapi::{
    registry::{MetaResponse, MetaResponses, Registry},
    ApiResponse,
};

pub type Response<A, T> = poem::Result<InnerResponse<A, T>>;

pub struct InnerResponse<A, T> {
    value: T,
    _auth: PhantomData<A>,
}

impl<A, T> From<T> for InnerResponse<A, T> {
    fn from(value: T) -> Self {
        Self {
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

impl<A, T> ApiResponse for InnerResponse<A, T>
where
    A: MetaResponsesExt,
    T: ApiResponse,
{
    fn meta() -> MetaResponses {
        let MetaResponses { mut responses } = T::meta();
        responses.extend(A::responses());
        MetaResponses { responses }
    }

    fn register(registry: &mut Registry) {
        T::register(registry);
        A::register(registry);
    }
}

impl<A, T> IntoResponse for InnerResponse<A, T>
where
    A: Send,
    T: IntoResponse,
{
    fn into_response(self) -> poem::Response {
        self.value.into_response()
    }
}
