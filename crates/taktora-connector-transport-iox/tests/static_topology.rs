//! TEST_0201 — static topology mode. `ServiceFactory::create_all`
//! pre-creates declared services with explicit QoS and rejects
//! undeclared services. Covers `TSR_0007` / `AFSR_0002`.

#![allow(clippy::doc_markdown)]

mod common;

use common::{Msg, TestJsonCodec, make_node, unique_channel_name};
use taktora_connector_core::{ChannelDescriptor, ConnectorError};
use taktora_connector_transport_iox::{ChannelSpec, ServiceFactory};

/// TEST_0201.1 — `create_all` creates every declared service with its
/// configured QoS and the declared services are subsequently openable.
#[test]
fn create_all_succeeds_and_services_openable() {
    let node = make_node();
    let mut factory = ServiceFactory::new(&node);
    let ch1_string = unique_channel_name("static_a");
    let ch2_string = unique_channel_name("static_b");
    // Leak strings to get &'static str for ChannelSpec (test-only)
    let ch1: &'static str = Box::leak(ch1_string.clone().into_boxed_str());
    let ch2: &'static str = Box::leak(ch2_string.clone().into_boxed_str());

    let topology = &[
        ChannelSpec {
            name: ch1,
            max_publishers: 1,
            max_subscribers: 2,
            subscriber_max_buffer_size: 32,
            history_size: 1,
        },
        ChannelSpec {
            name: ch2,
            max_publishers: 1,
            max_subscribers: 1,
            subscriber_max_buffer_size: 64,
            history_size: 2,
        },
    ];

    // Create all declared services
    factory.create_all::<1024>(topology).expect("create_all");

    // Declared services must be openable
    let desc1 = ChannelDescriptor::new(ch1_string, common::TestRouting).expect("non-empty");
    let desc2 = ChannelDescriptor::new(ch2_string, common::TestRouting).expect("non-empty");

    let writer1 = factory
        .create_writer::<Msg, _, _, 1024>(&desc1, TestJsonCodec)
        .expect("create_writer for declared ch1");

    let reader1 = factory
        .create_reader::<Msg, _, _, 1024>(&desc1, TestJsonCodec)
        .expect("create_reader for declared ch1");

    let _writer2 = factory
        .create_writer::<Msg, _, _, 1024>(&desc2, TestJsonCodec)
        .expect("create_writer for declared ch2");

    let _reader2 = factory
        .create_reader::<Msg, _, _, 1024>(&desc2, TestJsonCodec)
        .expect("create_reader for declared ch2");

    // Smoke test: send and receive on one declared channel
    writer1
        .send(&Msg {
            value: 42,
            note: "test".to_owned(),
        })
        .expect("send");

    for _ in 0..128 {
        if let Some(env) = reader1.try_recv().expect("try_recv") {
            assert_eq!(env.value.value, 42);
            assert_eq!(env.value.note, "test");
            return; // success
        }
        std::thread::yield_now();
    }
    panic!("envelope not delivered within retry budget");
}

/// TEST_0201.2 — after `create_all`, opening or creating an UNDECLARED
/// service is rejected with [`ConnectorError::Configuration`].
#[test]
fn undeclared_service_rejected_after_create_all() {
    let node = make_node();
    let mut factory = ServiceFactory::new(&node);

    let declared_string = unique_channel_name("declared");
    let undeclared_string = unique_channel_name("undeclared");
    let declared: &'static str = Box::leak(declared_string.clone().into_boxed_str());
    let undeclared: &'static str = Box::leak(undeclared_string.clone().into_boxed_str());

    let topology = &[ChannelSpec {
        name: declared,
        max_publishers: 1,
        max_subscribers: 1,
        subscriber_max_buffer_size: 64,
        history_size: 1,
    }];
    factory.create_all::<1024>(topology).expect("create_all");

    // Declared service is OK
    let desc_declared =
        ChannelDescriptor::new(declared_string, common::TestRouting).expect("non-empty");
    let _ok = factory
        .create_writer::<Msg, _, _, 1024>(&desc_declared, TestJsonCodec)
        .expect("create_writer for declared");

    let desc_undeclared =
        ChannelDescriptor::new(undeclared_string, common::TestRouting).expect("non-empty");
    let Err(err) = factory.create_writer::<Msg, _, _, 1024>(&desc_undeclared, TestJsonCodec) else {
        panic!("create_writer for undeclared should have failed");
    };

    match err {
        ConnectorError::Configuration(msg) => {
            assert!(
                msg.contains("not in declared topology"),
                "error message should mention topology rejection: {msg}"
            );
        }
        other => panic!("expected Configuration error, got {other:?}"),
    }

    // Undeclared reader also rejected
    let Err(err_reader) = factory.create_reader::<Msg, _, _, 1024>(&desc_undeclared, TestJsonCodec)
    else {
        panic!("create_reader for undeclared should have failed");
    };

    match err_reader {
        ConnectorError::Configuration(msg) => {
            assert!(msg.contains("not in declared topology"));
        }
        other => panic!("expected Configuration error, got {other:?}"),
    }

    // Undeclared raw writer also rejected
    let Err(err_raw_writer) = factory.create_raw_writer_named::<1024>(undeclared) else {
        panic!("create_raw_writer_named for undeclared should have failed");
    };

    match err_raw_writer {
        ConnectorError::Configuration(msg) => {
            assert!(msg.contains("not in declared topology"));
        }
        other => panic!("expected Configuration error, got {other:?}"),
    }

    // Undeclared raw reader also rejected
    let Err(err_raw_reader) = factory.create_raw_reader_named::<1024>(undeclared) else {
        panic!("create_raw_reader_named for undeclared should have failed");
    };

    match err_raw_reader {
        ConnectorError::Configuration(msg) => {
            assert!(msg.contains("not in declared topology"));
        }
        other => panic!("expected Configuration error, got {other:?}"),
    }
}

/// TEST_0201.3 — single-publisher constraint is explicitly configured
/// (`max_publishers(1)`). Attempting to create a second publisher on the
/// same service fails, proving the constraint is enforced by iceoryx2.
#[test]
fn single_publisher_constraint_enforced() {
    let node = make_node();
    let mut factory = ServiceFactory::new(&node);

    let ch_string = unique_channel_name("single_pub");
    let ch: &'static str = Box::leak(ch_string.clone().into_boxed_str());

    let topology = &[ChannelSpec {
        name: ch,
        max_publishers: 1,
        max_subscribers: 2,
        subscriber_max_buffer_size: 64,
        history_size: 1,
    }];
    factory.create_all::<1024>(topology).expect("create_all");

    let desc = ChannelDescriptor::new(ch_string, common::TestRouting).expect("non-empty");
    let _publisher1 = factory
        .create_writer::<Msg, _, _, 1024>(&desc, TestJsonCodec)
        .expect("first publisher");

    // Second publisher must fail (max_publishers = 1)
    let Err(err) = factory.create_writer::<Msg, _, _, 1024>(&desc, TestJsonCodec) else {
        panic!("second publisher should have failed");
    };

    // The error is a Stack error wrapping the iceoryx2 publisher creation
    // failure (exceeds max_publishers). The exact error variant and message
    // are iceoryx2 internals; we just verify it fails.
    match err {
        ConnectorError::Stack { .. } => {
            // Expected: iceoryx2 enforces the single-publisher limit
        }
        other => panic!("expected Stack error for exceeding max_publishers, got {other:?}"),
    }
}

/// TEST_0201.4 — without `create_all`, the factory remains in dynamic
/// mode and any service is openable (backwards compatibility).
#[test]
fn dynamic_mode_without_create_all() {
    let node = make_node();
    let factory = ServiceFactory::new(&node);

    let ch = unique_channel_name("dynamic");
    let desc = ChannelDescriptor::new(ch, common::TestRouting).expect("non-empty");

    // Dynamic mode: any service is openable without pre-declaration
    let writer = factory
        .create_writer::<Msg, _, _, 1024>(&desc, TestJsonCodec)
        .expect("dynamic create_writer");

    let reader = factory
        .create_reader::<Msg, _, _, 1024>(&desc, TestJsonCodec)
        .expect("dynamic create_reader");

    // Smoke test
    writer
        .send(&Msg {
            value: 99,
            note: "dynamic".to_owned(),
        })
        .expect("send");

    for _ in 0..128 {
        if let Some(env) = reader.try_recv().expect("try_recv") {
            assert_eq!(env.value.value, 99);
            return;
        }
        std::thread::yield_now();
    }
    panic!("envelope not delivered within retry budget");
}
