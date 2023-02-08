use std::borrow::Cow;

use poem_openapi::{
    registry::MetaSchemaRef,
    types::{ParseFromJSON, ParseResult, ToJSON, Type},
};
use sea_orm::ActiveValue;

#[derive(Debug, Clone, Copy)]
pub enum PatchValue<T> {
    Set(T),
    Unchanged,
}

impl<T> PatchValue<T> {
    pub fn update(self, old: T) -> ActiveValue<T>
    where
        T: Into<sea_orm::Value>,
    {
        match self {
            Self::Set(x) => ActiveValue::Set(x),
            Self::Unchanged => ActiveValue::Unchanged(old),
        }
    }
}

impl<T> ParseFromJSON for PatchValue<T>
where
    T: ParseFromJSON,
{
    fn parse_from_json(
        value: Option<poem_openapi::__private::serde_json::Value>,
    ) -> ParseResult<Self> {
        match Option::<T>::parse_from_json(value) {
            Ok(Some(x)) => Ok(Self::Set(x)),
            Ok(None) => Ok(Self::Unchanged),
            Err(x) => Err(x.propagate()),
        }
    }
}

impl<T> ToJSON for PatchValue<T>
where
    T: ToJSON,
{
    fn to_json(&self) -> Option<poem_openapi::__private::serde_json::Value> {
        match self {
            Self::Set(x) => Some(x),
            Self::Unchanged => None,
        }
        .to_json()
    }
}

impl<T> Type for PatchValue<T>
where
    T: Type,
{
    const IS_REQUIRED: bool = false;

    type RawValueType = T::RawValueType;

    type RawElementValueType = T::RawElementValueType;

    fn name() -> Cow<'static, str> {
        format!("optional<{}>", T::name()).into()
    }

    fn schema_ref() -> MetaSchemaRef {
        T::schema_ref()
    }

    fn as_raw_value(&self) -> Option<&Self::RawValueType> {
        match self {
            Self::Set(value) => value.as_raw_value(),
            Self::Unchanged => None,
        }
    }

    fn raw_element_iter<'a>(
        &'a self,
    ) -> Box<dyn Iterator<Item = &'a Self::RawElementValueType> + 'a> {
        match self {
            Self::Set(value) => value.raw_element_iter(),
            Self::Unchanged => Box::new(std::iter::empty()),
        }
    }
}
