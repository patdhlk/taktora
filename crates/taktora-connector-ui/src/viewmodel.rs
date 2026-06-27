//! The [`ViewModel`] authoring trait and the [`ImageEnum`] lowering trait.
//!
//! A ViewModel is one fixed-layout POD struct published latest-value over a
//! single service. The connector never publishes the authored struct directly;
//! it publishes an **integer-lowered image** ([`ViewModel::Image`]) into a
//! seqlock cell on the RT path. Lowering every C-like enum field to its backing
//! integer is what makes a torn seqlock read safe: a torn read of an integer is
//! always a valid bit pattern, whereas a torn read of a real enum discriminant
//! is undefined behaviour.

use taktora_connector_ui_contract::{FieldType, ViewModelSchema};

/// A C-like (field-less) enum lowered to a backing integer for the image.
///
/// Implemented (usually via `#[derive(ImageEnum)]`) by every enum used as a
/// [`ViewModel`] or command-params field. The derive requires an explicit
/// integer `#[repr(...)]`.
///
/// # Out-of-range reconstruction
///
/// [`from_repr`](ImageEnum::from_repr) must be total: a backing integer that
/// matches no declared discriminant (which a torn or stale read could in
/// principle surface) is mapped to the **first declared variant**. This keeps
/// reconstruction infallible and deterministic; round-tripping any *in-range*
/// value through [`to_repr`](ImageEnum::to_repr) / `from_repr` is the identity.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be a ViewModel/command field",
    label = "not a supported field type",
    note = "ViewModel/command fields must be one of: bool, i8..i64, u8..u64, f32/f64, fixed arrays of those, BoundedString<CAP>, or a C-like enum deriving ImageEnum",
    note = "nested POD structs are part of REQ_0858 but are not yet supported by the derive (deferred)"
)]
pub trait ImageEnum: Copy + Sized {
    /// The backing integer the enum lowers to (e.g. `u8`).
    type Repr: Copy + Send + 'static;

    /// The `(variant name, discriminant)` pairs, in declaration order.
    const VARIANTS: &'static [(&'static str, i64)];

    /// The backing integer width in bytes.
    const WIDTH: u8;

    /// A conservative upper bound on the JSON-encoded byte length of any
    /// variant (used to size the publish envelope).
    const MAX_JSON: usize;

    /// The enum's Rust type name, used as the schema enum name.
    fn type_name() -> &'static str;

    /// Lower the value to its backing integer.
    fn to_repr(self) -> Self::Repr;

    /// Reconstruct from a backing integer, falling back to the first declared
    /// variant for any out-of-range value (see the trait docs).
    fn from_repr(repr: Self::Repr) -> Self;

    /// The contract field-type descriptor for this enum.
    #[must_use]
    fn field_type() -> FieldType {
        FieldType::Enum {
            name: Self::type_name().to_owned(),
            variants: Self::VARIANTS
                .iter()
                .map(|(n, d)| ((*n).to_owned(), *d))
                .collect(),
            width: Self::WIDTH,
        }
    }
}

/// An authored ViewModel: a fixed-layout POD struct published latest-value.
///
/// Implemented (usually via `#[derive(ViewModel)]`) by the application's
/// ViewModel structs. The associated [`Image`](ViewModel::Image) is a generated
/// `#[repr(C)]`, `Copy` struct with every enum field lowered to its backing
/// integer; it is the only thing written into the RT seqlock cell.
pub trait ViewModel: Sized {
    /// The integer-lowered, `#[repr(C)]` `Copy` image. Seqlock-safe.
    type Image: Copy + Send + 'static;

    /// `size_of::<Self::Image>()` — the number of bytes the seqlock cell holds.
    ///
    /// Kept as a plain associated `const` (not used as an array length) because
    /// stable Rust cannot use an associated const as a generic array length.
    const IMAGE_SIZE: usize;

    /// A conservative upper bound on the JSON-encoded byte length of the
    /// ViewModel, used to size the publish envelope.
    const MAX_ENCODED_SIZE: usize;

    /// The structural schema (name + fields). The `service` field is left empty
    /// for the connector to fill with the instance-namespaced service name.
    fn schema() -> ViewModelSchema;

    /// Lower `self` into its image (allocation-free; runs on the RT path).
    fn to_image(&self) -> Self::Image;

    /// Reconstruct the ViewModel from an image.
    fn from_image(image: &Self::Image) -> Self;

    /// Serialize the reconstructed ViewModel as JSON into `buf`.
    ///
    /// Runs **off the RT path** (in the pump); it may allocate.
    fn image_to_json(image: &Self::Image, buf: &mut Vec<u8>);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;
    use taktora_connector_ui_contract::FieldSchema;

    /// A C-like enum, hand-lowered to `u8` to exercise the trait before the
    /// derive exists.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
    #[repr(u8)]
    enum StepperState {
        Idle = 0,
        Running = 1,
        Faulted = 2,
    }

    impl ImageEnum for StepperState {
        type Repr = u8;
        const VARIANTS: &'static [(&'static str, i64)] =
            &[("Idle", 0), ("Running", 1), ("Faulted", 2)];
        const WIDTH: u8 = 1;
        const MAX_JSON: usize = 9; // "Faulted" quoted = 9 bytes.

        fn type_name() -> &'static str {
            "StepperState"
        }
        fn to_repr(self) -> u8 {
            self as u8
        }
        fn from_repr(repr: u8) -> Self {
            match repr {
                0 => Self::Idle,
                1 => Self::Running,
                2 => Self::Faulted,
                _ => Self::Idle, // out-of-range -> first variant
            }
        }
    }

    #[derive(Clone, Debug, PartialEq, Serialize)]
    struct StepperVm {
        position: f64,
        active: bool,
        state: StepperState,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct StepperVmImage {
        position: f64,
        active: bool,
        state: u8,
    }

    impl ViewModel for StepperVm {
        type Image = StepperVmImage;
        const IMAGE_SIZE: usize = core::mem::size_of::<StepperVmImage>();
        const MAX_ENCODED_SIZE: usize = 64;

        fn schema() -> ViewModelSchema {
            ViewModelSchema {
                name: "StepperVm".to_owned(),
                service: String::new(),
                fields: vec![
                    FieldSchema {
                        name: "position".to_owned(),
                        ty: FieldType::F64,
                    },
                    FieldSchema {
                        name: "active".to_owned(),
                        ty: FieldType::Bool,
                    },
                    FieldSchema {
                        name: "state".to_owned(),
                        ty: StepperState::field_type(),
                    },
                ],
            }
        }

        fn to_image(&self) -> StepperVmImage {
            StepperVmImage {
                position: self.position,
                active: self.active,
                state: self.state.to_repr(),
            }
        }

        fn from_image(image: &StepperVmImage) -> Self {
            Self {
                position: image.position,
                active: image.active,
                state: StepperState::from_repr(image.state),
            }
        }

        fn image_to_json(image: &StepperVmImage, buf: &mut Vec<u8>) {
            let vm = Self::from_image(image);
            serde_json::to_writer(buf, &vm).expect("ViewModel JSON is infallible for POD");
        }
    }

    #[test]
    fn image_size_matches_size_of_image() {
        assert_eq!(
            StepperVm::IMAGE_SIZE,
            core::mem::size_of::<StepperVmImage>()
        );
    }

    #[test]
    fn to_from_image_round_trips_all_in_range_values() {
        for state in [
            StepperState::Idle,
            StepperState::Running,
            StepperState::Faulted,
        ] {
            let vm = StepperVm {
                position: 12.5,
                active: true,
                state,
            };
            let image = vm.to_image();
            let back = StepperVm::from_image(&image);
            assert_eq!(back, vm);
        }
    }

    #[test]
    fn from_repr_out_of_range_falls_back_to_first_variant() {
        assert_eq!(StepperState::from_repr(99), StepperState::Idle);
    }

    #[test]
    fn schema_describes_the_fields() {
        let s = StepperVm::schema();
        assert_eq!(s.name, "StepperVm");
        assert_eq!(s.fields.len(), 3);
        assert_eq!(s.fields[0].ty, FieldType::F64);
        assert_eq!(s.fields[1].ty, FieldType::Bool);
        assert_eq!(
            s.fields[2].ty,
            FieldType::Enum {
                name: "StepperState".to_owned(),
                variants: vec![
                    ("Idle".to_owned(), 0),
                    ("Running".to_owned(), 1),
                    ("Faulted".to_owned(), 2),
                ],
                width: 1,
            }
        );
    }

    #[test]
    fn image_to_json_serializes_reconstructed_view_model() {
        let vm = StepperVm {
            position: 3.0,
            active: false,
            state: StepperState::Running,
        };
        let image = vm.to_image();
        let mut buf = Vec::new();
        StepperVm::image_to_json(&image, &mut buf);
        let json: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(json["position"], 3.0);
        assert_eq!(json["active"], false);
        assert_eq!(json["state"], "Running");
    }
}
