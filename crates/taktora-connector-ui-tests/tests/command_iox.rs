//! Live iceoryx2 round-trip tests for the UI connector's command plane: the
//! production [`IoxCommandTransport`] driven by a [`CommandHandler`] against a
//! UI-shaped client that publishes invocations on the request service and reads
//! acks on the reply service (`REQ_0865`, `REQ_0867`, `REQ_0870`).
//!
//! These stand up real iceoryx2 services (shared memory), so they live in the
//! `publish = false` `-tests` crate; the deterministic behaviour (dedupe,
//! back-pressure, gating, unknown command) is covered by the unit tests over
//! `MockCommandTransport`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use iceoryx2::node::Node;
use iceoryx2::prelude::{NodeBuilder, ipc};
use serde::Deserialize;
use taktora_connector_transport_iox::{RawChannelReader, ServiceFactory};
use taktora_connector_ui::contract::Ack;
use taktora_connector_ui::{
    CanExecute, CommandHandler, CommandTransport, IoxCommandTransport, command_channel,
};

const N: usize = 256;

#[derive(Clone, Debug, PartialEq, Deserialize)]
struct Jog {
    delta: f64,
}

fn make_node() -> Node<ipc::Service> {
    NodeBuilder::new()
        .create::<ipc::Service>()
        .expect("create iceoryx2 node")
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique(tag: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("uicmd.{tag}.{n}")
}

fn corr(byte: u8) -> [u8; 32] {
    let mut id = [0u8; 32];
    id[0] = byte;
    id
}

/// Drain one reply envelope, polling briefly for delivery.
fn recv_reply(reader: &RawChannelReader<N>) -> Option<Vec<u8>> {
    let mut dest = [0u8; N];
    for _ in 0..200 {
        if let Ok(Some(sample)) = reader.try_recv_into(&mut dest) {
            return Some(dest[..sample.payload_len].to_vec());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    None
}

/// Poll the handler until it has handled at least one invocation (or give up).
fn poll_until_handled<T: CommandTransport>(handler: &mut CommandHandler<T>) {
    for _ in 0..200 {
        if handler.poll() > 0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn command_round_trips_accepted_over_iox() {
    let node = make_node();
    let req = unique("jog.req");
    let rep = unique("jog.rep");
    let factory = ServiceFactory::new(&node);

    // Connector side: the request reader must subscribe before the client
    // publishes, and the reply writer publishes the ack.
    let handler_reader = factory
        .create_raw_reader_named::<N>(&req)
        .expect("req reader");
    let reply_writer = factory
        .create_raw_writer_named::<N>(&rep)
        .expect("rep writer");

    let can = CanExecute::new(true);
    let (command, effects) = command_channel::<Jog>(&can, 8);
    let mut transport = IoxCommandTransport::<N>::new();
    transport.add_command("jog", handler_reader, reply_writer);
    let mut handler = CommandHandler::new(transport, 16);
    handler.register("jog", command);

    // UI side: publish the invocation, subscribe to the reply BEFORE the handler
    // sends it.
    let req_writer = factory
        .create_raw_writer_named::<N>(&req)
        .expect("client req writer");
    let rep_reader = factory
        .create_raw_reader_named::<N>(&rep)
        .expect("client rep reader");

    req_writer
        .send_raw_bytes(b"{\"delta\":2.5}", corr(1))
        .expect("send invocation");

    poll_until_handled(&mut handler);

    // The effect was enqueued (off-RT, not run inline).
    assert_eq!(effects.try_recv().ok(), Some(Jog { delta: 2.5 }));

    // The UI receives an Accepted ack.
    let bytes = recv_reply(&rep_reader).expect("ack delivered");
    let ack: Ack = serde_json::from_slice(&bytes).expect("ack parses");
    assert_eq!(ack, Ack::Accepted);
}

#[test]
fn retry_with_same_correlation_id_dedupes_over_iox() {
    let node = make_node();
    let req = unique("dedupe.req");
    let rep = unique("dedupe.rep");
    let factory = ServiceFactory::new(&node);

    let handler_reader = factory
        .create_raw_reader_named::<N>(&req)
        .expect("req reader");
    let reply_writer = factory
        .create_raw_writer_named::<N>(&rep)
        .expect("rep writer");

    let can = CanExecute::new(true);
    let (command, effects) = command_channel::<Jog>(&can, 8);
    let mut transport = IoxCommandTransport::<N>::new();
    transport.add_command("jog", handler_reader, reply_writer);
    let mut handler = CommandHandler::new(transport, 16);
    handler.register("jog", command);

    let req_writer = factory
        .create_raw_writer_named::<N>(&req)
        .expect("client req writer");
    let rep_reader = factory
        .create_raw_reader_named::<N>(&rep)
        .expect("client rep reader");

    // Two sends under the SAME correlation id (a UI retry).
    req_writer
        .send_raw_bytes(b"{\"delta\":4.0}", corr(9))
        .expect("send 1");
    poll_until_handled(&mut handler);
    let ack1: Ack = serde_json::from_slice(&recv_reply(&rep_reader).expect("ack 1")).unwrap();
    assert_eq!(ack1, Ack::Accepted);

    req_writer
        .send_raw_bytes(b"{\"delta\":4.0}", corr(9))
        .expect("send 2 (retry)");
    poll_until_handled(&mut handler);
    let ack2: Ack = serde_json::from_slice(&recv_reply(&rep_reader).expect("ack 2")).unwrap();
    assert_eq!(ack2, Ack::Accepted, "retry replays the cached ack");

    // ...but the effect was enqueued exactly once (dedupe suppressed re-enqueue).
    assert_eq!(effects.try_recv().ok(), Some(Jog { delta: 4.0 }));
    assert!(
        effects.try_recv().is_err(),
        "retry under the same correlation id must not re-enqueue"
    );
}
