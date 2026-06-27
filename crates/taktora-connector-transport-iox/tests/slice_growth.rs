//! TEST_0885 — slice channel data-segment growth and ceiling enforcement.
//!
//! Verifies REQ_0887 (start at `initial_max_slice_len`, grow by
//! `AllocationStrategy::PowerOfTwo`) and REQ_0888 (reject a loan exceeding
//! `max_payload_bytes` with a bounded-capacity `ConnectorError` rather than
//! growing past the ceiling).

#![allow(clippy::doc_markdown)]

mod common;

use common::make_node;
use taktora_connector_core::ConnectorError;
use taktora_connector_transport_iox::ServiceFactory;

#[test]
fn slice_growth_and_ceiling() {
    const INITIAL: usize = 16;
    const CEILING: usize = 4096;

    let node = make_node();
    let factory = ServiceFactory::new(&node);
    let name = common::unique_channel_name("slice_grow");

    let reader = factory
        .create_slice_reader(&name)
        .expect("create slice reader");
    let writer = factory
        .create_slice_writer(&name, INITIAL, CEILING)
        .expect("create slice writer");

    // Increasing sizes that cross the initial length, forcing PowerOfTwo
    // growth of the data segment. All are <= the ceiling and must deliver.
    let sizes: [usize; 5] = [8, 16, 33, 100, CEILING];
    for (i, &len) in sizes.iter().enumerate() {
        let payload: Vec<u8> = (0..len)
            .map(|n| u8::try_from((n + i) % 256).unwrap())
            .collect();
        let outcome = writer
            .send(&payload)
            .unwrap_or_else(|e| panic!("send of {len} bytes should succeed, got {e:?}"));
        assert_eq!(outcome.bytes_written, len);

        let recv = reader
            .try_recv()
            .expect("try_recv")
            .expect("a sample is available");
        assert_eq!(recv.payload(), payload.as_slice());
        assert_eq!(recv.payload().len(), len);
    }

    // A payload above the ceiling is refused BEFORE loaning — no further
    // growth.
    let too_big = vec![0u8; CEILING + 1];
    match writer.send(&too_big) {
        Err(ConnectorError::PayloadOverflow { actual, max }) => {
            assert_eq!(actual, CEILING + 1);
            assert_eq!(max, CEILING);
        }
        other => panic!("expected PayloadOverflow, got {other:?}"),
    }
}
