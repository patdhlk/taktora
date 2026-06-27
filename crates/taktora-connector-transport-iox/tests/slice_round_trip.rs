//! TEST_0884 — `SliceChannelWriter` → `SliceChannelReader` round-trip via
//! a real iceoryx2 `[u8]` pub/sub service. Verifies REQ_0885, REQ_0886 and
//! REQ_0889: variable-length zero-copy delivery where each sample carries
//! exactly its message bytes, plus monotonic `sequence_number` and
//! non-decreasing `timestamp_ns` read off the iceoryx2 user-header.

#![allow(clippy::doc_markdown)]

mod common;

use common::make_node;
use taktora_connector_transport_iox::ServiceFactory;

#[test]
fn slice_writer_to_reader_round_trip() {
    let node = make_node();
    let factory = ServiceFactory::new(&node);
    let name = common::unique_channel_name("slice_rt");

    // Reader (subscriber) first so it is attached before the first send.
    let reader = factory
        .create_slice_reader(&name)
        .expect("create slice reader");
    let writer = factory
        .create_slice_writer(&name, 64, 4096)
        .expect("create slice writer");

    // Messages of deliberately DIFFERING lengths.
    let messages: Vec<Vec<u8>> = vec![
        b"hi".to_vec(),
        b"a slightly longer message".to_vec(),
        (0u8..100).collect(),
        vec![0xABu8; 7],
        (0u8..200).map(|n| n.wrapping_mul(3)).collect(),
    ];

    for msg in &messages {
        let outcome = writer.send(msg).expect("send slice");
        assert_eq!(outcome.bytes_written, msg.len());
    }

    let mut last_ts: u64 = 0;
    for (expected_seq, msg) in messages.iter().enumerate() {
        let recv = reader
            .try_recv()
            .expect("try_recv")
            .expect("a sample is available");

        // Payload bytes round-trip exactly and length equals the message
        // length (not a fixed N).
        assert_eq!(recv.payload(), msg.as_slice());
        assert_eq!(recv.payload().len(), msg.len());

        // Sequence number increments monotonically from zero.
        assert_eq!(recv.sequence_number(), expected_seq as u64);

        // Timestamp is non-decreasing across samples.
        assert!(recv.timestamp_ns() >= last_ts);
        last_ts = recv.timestamp_ns();
    }

    // Queue drained.
    assert!(reader.try_recv().expect("try_recv empty").is_none());
}
