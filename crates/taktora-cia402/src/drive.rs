//! Process-image accessors for a `CiA` 402 `CSP` drive (`REQ_0859`).
//!
//! The ESI codegen emits an impl per `CiA` 402 device (`BB_0099`), matched by
//! `CoE` index (`0x6040`/`0x6041`/`0x6060`/`0x607A`/`0x6064`).
//! `Image` is the connector's process-image type; `[u8]` for the mock.

/// Process-image accessors for a `CiA` 402 `CSP` drive.
pub trait Cia402Drive {
    /// The connector's process-image type (e.g. a PDI byte slice).
    type Image: ?Sized;

    /// Read object `0x6041` (`statusword`) from the input image.
    fn statusword(&self, img: &Self::Image) -> u16;
    /// Write object `0x6040` (`controlword`) into the output image.
    fn set_controlword(&self, img: &mut Self::Image, cw: u16);
    /// Read object `0x6064` (position actual value) from the input image.
    fn actual_position(&self, img: &Self::Image) -> i32;
    /// Write object `0x607A` (target position) into the output image.
    fn set_target_position(&self, img: &mut Self::Image, p: i32);
    /// Write object `0x6060` (modes of operation; 8 = `CSP`).
    fn set_mode(&self, img: &mut Self::Image, mode: u8);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Image layout for the test drive: [cw:2][mode:1][target:4] outputs,
    // [sw:2][actual:4] inputs — collapsed into a single flat buffer here for
    // simplicity (a real PDI / Task 9's `MockDrive` uses split in/out slices).
    struct TestDrive;
    impl Cia402Drive for TestDrive {
        type Image = [u8];
        fn statusword(&self, img: &[u8]) -> u16 {
            u16::from_le_bytes([img[7], img[8]])
        }
        fn set_controlword(&self, img: &mut [u8], v: u16) {
            img[0..2].copy_from_slice(&v.to_le_bytes());
        }
        fn actual_position(&self, img: &[u8]) -> i32 {
            i32::from_le_bytes([img[9], img[10], img[11], img[12]])
        }
        fn set_target_position(&self, img: &mut [u8], p: i32) {
            img[3..7].copy_from_slice(&p.to_le_bytes());
        }
        fn set_mode(&self, img: &mut [u8], m: u8) {
            img[2] = m;
        }
    }

    #[test]
    fn round_trips_through_the_image() {
        let mut img = [0u8; 13];
        let d = TestDrive;
        d.set_controlword(&mut img, 0x000F);
        d.set_mode(&mut img, 8);
        d.set_target_position(&mut img, -1234);
        img[7..9].copy_from_slice(&0x0237u16.to_le_bytes());
        img[9..13].copy_from_slice(&(-1234i32).to_le_bytes());
        assert_eq!(u16::from_le_bytes([img[0], img[1]]), 0x000F);
        assert_eq!(d.statusword(&img), 0x0237);
        assert_eq!(d.actual_position(&img), -1234);
        assert_eq!(img[2], 8);
    }
}
