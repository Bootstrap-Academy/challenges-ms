use std::marker::PhantomData;

use poem::IntoResponse;
use poem_openapi::{
    registry::{MetaResponse, MetaResponses, Registry},
    ApiResponse,
};

pub type Response<T, A = ()> = poem::Result<InnerResponse<T, A>>;

pub struct InnerResponse<T, A> {
    value: T,
    _auth: PhantomData<A>,
}

impl<T, A> From<T> for InnerResponse<T, A> {
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

impl<T, A> ApiResponse for InnerResponse<T, A>
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

impl<T, A> IntoResponse for InnerResponse<T, A>
where
    A: Send,
    T: IntoResponse,
{
    fn into_response(self) -> poem::Response {
        self.value.into_response()
    }
}
