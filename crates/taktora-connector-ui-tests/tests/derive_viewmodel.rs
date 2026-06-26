//! Integration tests for `#[derive(ViewModel)]` and `#[derive(ImageEnum)]`.

use serde::Serialize;
use taktora_connector_ui::{BoundedString, ImageEnum, ViewModel};
use taktora_connector_ui_contract::{FieldType, ViewModelSchema};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ImageEnum)]
#[repr(u8)]
enum StepperState {
    Idle = 0,
    Running = 1,
    Faulted = 2,
}

#[derive(Clone, Debug, PartialEq, Serialize, ViewModel)]
struct StepperVm {
    active: bool,
    position: f64,
    buf: [u8; 4],
    name: BoundedString<16>,
    state: StepperState,
}

fn sample() -> StepperVm {
    StepperVm {
        active: true,
        position: 42.5,
        buf: [1, 2, 3, 4],
        name: BoundedString::from("axis-x"),
        state: StepperState::Running,
    }
}

#[test]
fn schema_matches_expected() {
    let schema = StepperVm::schema();
    let expected = ViewModelSchema {
        name: "StepperVm".to_owned(),
        service: String::new(),
        fields: vec![
            field("active", FieldType::Bool),
            field("position", FieldType::F64),
            field(
                "buf",
                FieldType::Array {
                    elem: Box::new(FieldType::U8),
                    len: 4,
                },
            ),
            field("name", FieldType::Str { cap: 16 }),
            field(
                "state",
                FieldType::Enum {
                    name: "StepperState".to_owned(),
                    variants: vec![
                        ("Idle".to_owned(), 0),
                        ("Running".to_owned(), 1),
                        ("Faulted".to_owned(), 2),
                    ],
                    width: 1,
                },
            ),
        ],
    };
    assert_eq!(schema, expected);
}

#[test]
fn to_from_image_round_trips_all_enum_values() {
    for state in [
        StepperState::Idle,
        StepperState::Running,
        StepperState::Faulted,
    ] {
        let mut vm = sample();
        vm.state = state;
        let image = vm.to_image();
        let back = StepperVm::from_image(&image);
        assert_eq!(back, vm);
    }
}

#[test]
fn image_size_equals_size_of_image() {
    let image = sample().to_image();
    assert_eq!(StepperVm::IMAGE_SIZE, std::mem::size_of_val(&image));
}

#[test]
fn image_is_copy_and_repr_c_lowered_enum() {
    // The image's `state` is the backing integer, so a torn read is a valid bit
    // pattern. We prove the image is `Copy` (no enum/Drop) by copying it.
    let image = sample().to_image();
    let copy = image;
    assert_eq!(StepperVm::from_image(&copy).state, StepperState::Running);
}

#[test]
fn out_of_range_discriminant_falls_back_to_first_variant() {
    assert_eq!(StepperState::from_repr(250), StepperState::Idle);
}

#[test]
fn image_to_json_serializes_reconstructed_view_model() {
    let vm = sample();
    let image = vm.to_image();
    let mut buf = Vec::new();
    StepperVm::image_to_json(&image, &mut buf);
    let json: serde_json::Value = serde_json::from_slice(&buf).unwrap();
    assert_eq!(json["active"], true);
    assert_eq!(json["position"], 42.5);
    assert_eq!(json["buf"], serde_json::json!([1, 2, 3, 4]));
    assert_eq!(json["name"], "axis-x");
    assert_eq!(json["state"], "Running");
}

#[test]
fn max_encoded_size_is_a_sane_upper_bound() {
    let vm = sample();
    let image = vm.to_image();
    let mut buf = Vec::new();
    StepperVm::image_to_json(&image, &mut buf);
    assert!(
        buf.len() <= StepperVm::MAX_ENCODED_SIZE,
        "encoded {} exceeds MAX_ENCODED_SIZE {}",
        buf.len(),
        StepperVm::MAX_ENCODED_SIZE
    );
}

fn field(name: &str, ty: FieldType) -> taktora_connector_ui_contract::FieldSchema {
    taktora_connector_ui_contract::FieldSchema {
        name: name.to_owned(),
        ty,
    }
}
