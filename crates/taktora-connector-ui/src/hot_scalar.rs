//! [`HotScalar<T>`]: a single-scalar ViewModel published on its **own** service
//! (`REQ_0863`).
//!
//! The default MVVM granularity is one struct per ViewModel per service. A hot
//! scalar opts a single fast-changing value onto its own service so a UI can
//! subscribe to *just* that field at the UI cadence without re-reading (and
//! re-diffing) the whole struct every tick.
//!
//! # Scope (v1 minimal form)
//!
//! This is the documented minimal form of `REQ_0863`: it is a **standalone**
//! single-scalar ViewModel registered on its own service via
//! [`UiConnector::add_hot_scalar`](crate::UiConnector::add_hot_scalar) and listed
//! as its own entry in the manifest. It is **not** a field carved out of an
//! existing multi-field ViewModel struct (full field-promotion is deferred), and
//! the manifest does not carry an explicit `hot` boolean — the observable
//! contract is simply "this scalar lives on its own service", which is what a UI
//! needs to subscribe to it independently. Adding a dedicated `hot` flag to
//! [`ViewModelSchema`] is
//! deferred to avoid a breaking change to the (already golden-fixtured) contract
//! crate.

use serde::Serialize;
use taktora_connector_ui_contract::{FieldSchema, FieldType, ViewModelSchema};

use crate::viewmodel::ViewModel;

/// A POD scalar that may be promoted to its own service via [`HotScalar`].
///
/// Implemented for the closed set of supported scalar field types (`bool`, the
/// fixed-width integers, and the floats); each carries its contract
/// [`FieldType`] and a conservative JSON upper bound.
pub trait HotScalarValue: Copy + Send + 'static + Serialize {
    /// The contract field-type descriptor for this scalar.
    const FIELD_TYPE: FieldType;
    /// A conservative upper bound on the JSON-encoded byte length of the value.
    const MAX_JSON: usize;
}

macro_rules! impl_hot_scalar_value {
    ($($t:ty => $ft:expr, $max:expr;)*) => {$(
        impl HotScalarValue for $t {
            const FIELD_TYPE: FieldType = $ft;
            const MAX_JSON: usize = $max;
        }
    )*};
}

impl_hot_scalar_value! {
    bool => FieldType::Bool, 5;
    i8   => FieldType::I8,  4;
    i16  => FieldType::I16, 6;
    i32  => FieldType::I32, 11;
    i64  => FieldType::I64, 20;
    u8   => FieldType::U8,  3;
    u16  => FieldType::U16, 5;
    u32  => FieldType::U32, 10;
    u64  => FieldType::U64, 20;
    f32  => FieldType::F32, 32;
    f64  => FieldType::F64, 32;
}

/// A single-scalar ViewModel with one field named `value` (`REQ_0863`).
///
/// Published on its own latest-value service. The RT path drives it through a
/// [`Property<HotScalar<T>>`](crate::Property), exactly like any other ViewModel.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct HotScalar<T> {
    /// The promoted scalar value.
    pub value: T,
}

impl<T> HotScalar<T> {
    /// Wrap a scalar value.
    pub const fn new(value: T) -> Self {
        Self { value }
    }
}

/// The `#[repr(C)] Copy` image of a [`HotScalar`] — a single scalar, already a
/// valid bit pattern under a torn seqlock read (no enum discriminant).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HotScalarImage<T: Copy> {
    value: T,
}

impl<T: HotScalarValue> ViewModel for HotScalar<T> {
    type Image = HotScalarImage<T>;
    const IMAGE_SIZE: usize = core::mem::size_of::<HotScalarImage<T>>();
    // `{"value":<v>}` — 10 framing bytes plus the value's JSON upper bound.
    const MAX_ENCODED_SIZE: usize = T::MAX_JSON + 16;

    fn schema() -> ViewModelSchema {
        ViewModelSchema {
            // The connector overwrites `name` with the registered name; the
            // single field is always `value`.
            name: String::new(),
            service: String::new(),
            fields: vec![FieldSchema {
                name: "value".to_owned(),
                ty: T::FIELD_TYPE,
            }],
        }
    }

    fn to_image(&self) -> HotScalarImage<T> {
        HotScalarImage { value: self.value }
    }

    fn from_image(image: &HotScalarImage<T>) -> Self {
        Self { value: image.value }
    }

    fn image_to_json(image: &HotScalarImage<T>, buf: &mut Vec<u8>) {
        let vm = Self::from_image(image);
        serde_json::to_writer(buf, &vm).expect("hot-scalar JSON is infallible for POD");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::property::Property;

    #[test]
    fn schema_has_one_value_field_of_the_scalar_type() {
        let s = <HotScalar<f64> as ViewModel>::schema();
        assert_eq!(s.fields.len(), 1);
        assert_eq!(s.fields[0].name, "value");
        assert_eq!(s.fields[0].ty, FieldType::F64);
    }

    #[test]
    fn round_trips_through_image_and_property() {
        let prop = Property::<HotScalar<f64>>::new();
        let reader = prop.reader();
        prop.set(&HotScalar::new(3.5));
        let got = reader.snapshot().expect("a value was set");
        assert_eq!(got.value, 3.5);
    }

    #[test]
    fn image_to_json_is_a_value_object() {
        let img = HotScalar::new(7u32).to_image();
        let mut buf = Vec::new();
        HotScalar::<u32>::image_to_json(&img, &mut buf);
        let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(v["value"], 7);
    }
}
