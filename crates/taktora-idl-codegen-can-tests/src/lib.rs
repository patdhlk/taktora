//! Verification crate for the message-plane CAN codegen.
//!
//! The generated module (`build.rs` → `$OUT_DIR/vehicle.rs`) is compiled into
//! this crate and round-tripped in the tests below. This is the slice's
//! proof-of-life: generated `WireType` code that actually compiles and whose
//! `encode`/`decode` agree (`TEST_0880`+).

#![allow(dead_code)]

include!(concat!(env!("OUT_DIR"), "/vehicle.rs"));

#[cfg(test)]
mod tests {
    use crate::vehicle::{BodyControl, EngineData, EngineDataGear};
    use taktora_idl_wire::{WireError, WireType};

    #[test]
    fn max_serialized_len_matches_dlc() {
        // EngineData is an 8-byte frame; BodyControl is 2.
        assert_eq!(EngineData::MAX_SERIALIZED_LEN, 8);
        assert_eq!(BodyControl::MAX_SERIALIZED_LEN, 2);
    }

    #[test]
    fn engine_data_round_trips() {
        let msg = EngineData {
            rpm: 12_000,
            coolant_temp: -10,
            gear: EngineDataGear::First,
        };
        let mut buf = [0u8; 8];
        let n = msg.encode(&mut buf).unwrap();
        assert_eq!(n, 8);

        let back = EngineData::decode(&buf).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn little_endian_signal_placement_is_correct() {
        let msg = EngineData {
            rpm: 0x1234,
            coolant_temp: 0,
            gear: EngineDataGear::Neutral,
        };
        let mut buf = [0u8; 8];
        msg.encode(&mut buf).unwrap();
        // Rpm is a 16-bit little-endian signal at bit 0: LSB in byte 0.
        assert_eq!(buf[0], 0x34);
        assert_eq!(buf[1], 0x12);
    }

    #[test]
    fn enum_round_trips_and_rejects_unknown() {
        let msg = EngineData {
            rpm: 0,
            coolant_temp: 0,
            gear: EngineDataGear::Second,
        };
        let mut buf = [0u8; 8];
        msg.encode(&mut buf).unwrap();
        assert_eq!(
            EngineData::decode(&buf).unwrap().gear,
            EngineDataGear::Second
        );

        // Force the 4-bit Gear field (bits 24..28, i.e. low nibble of byte 3)
        // to 7 — a value with no defined variant.
        buf[3] = 0x07;
        assert_eq!(EngineData::decode(&buf), Err(WireError::UnknownEnumValue));
    }

    #[test]
    fn buffer_too_small_is_rejected() {
        let msg = EngineData {
            rpm: 1,
            coolant_temp: 1,
            gear: EngineDataGear::Neutral,
        };
        let mut buf = [0u8; 4];
        assert_eq!(msg.encode(&mut buf), Err(WireError::BufferTooSmall));
        assert_eq!(EngineData::decode(&buf), Err(WireError::BufferTooSmall));
    }

    #[test]
    fn multiplexed_signals_flatten_into_one_struct() {
        // Mux + DoorState both become plain fields in this slice.
        let msg = BodyControl {
            mux: 0,
            door_state: 5,
        };
        let mut buf = [0u8; 2];
        msg.encode(&mut buf).unwrap();
        assert_eq!(BodyControl::decode(&buf).unwrap(), msg);
    }
}
