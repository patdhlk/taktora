//! Live iceoryx2 round-trip tests for the UI connector's publish plane: the
//! [`IoxVmPublisher`] production [`VmPublisher`] and the [`Pump`] driving it
//! (`REQ_0856`, `REQ_0861`, `REQ_0862`).
//!
//! These stand up a real iceoryx2 service (shared memory), so they live in the
//! `publish = false` `-tests` crate rather than the unit tests, which use the
//! `MockPublisher`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use iceoryx2::node::Node;
use iceoryx2::prelude::{NodeBuilder, ipc};
use serde::Serialize;
use taktora_connector_transport_iox::ServiceFactory;
use taktora_connector_ui::pump::{Pump, VmPublisher, property_entry};
use taktora_connector_ui::{ImageEnum, IoxVmPublisher, Property, ViewModel};

const N: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ImageEnum)]
#[repr(u8)]
enum StepperState {
    Idle = 0,
    Running = 1,
}

#[derive(Clone, Debug, PartialEq, Serialize, ViewModel)]
struct StepperVm {
    active: bool,
    position: f64,
    state: StepperState,
}

fn make_node() -> Node<ipc::Service> {
    NodeBuilder::new()
        .create::<ipc::Service>()
        .expect("create iceoryx2 node")
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_name(tag: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("uipump.{tag}.{n}")
}

/// Read one envelope, polling briefly for delivery.
fn recv(reader: &taktora_connector_transport_iox::RawChannelReader<N>) -> Option<Vec<u8>> {
    let mut dest = [0u8; N];
    for _ in 0..200 {
        if let Ok(Some(sample)) = reader.try_recv_into(&mut dest) {
            return Some(dest[..sample.payload_len].to_vec());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    None
}

#[test]
fn subscriber_count_tracks_attached_readers() {
    let node = make_node();
    let name = unique_name("subcount");
    let publisher = IoxVmPublisher::<N>::create(&node, &name).expect("create publisher");

    assert_eq!(publisher.subscriber_count(), 0);

    let factory = ServiceFactory::new(&node);
    let _reader = factory
        .create_raw_reader_named::<N>(&name)
        .expect("open same service as a reader");
    assert_eq!(publisher.subscriber_count(), 1);
}

#[test]
fn pump_publishes_view_model_json_over_iox() {
    let node = make_node();
    let name = unique_name("vm");

    // Reader attaches first so the pump sees a subscriber (non-exempt entry).
    let publisher = IoxVmPublisher::<N>::create(&node, &name).expect("create publisher");
    let factory = ServiceFactory::new(&node);
    let reader = factory.create_raw_reader_named::<N>(&name).expect("reader");

    let prop = Property::<StepperVm>::new();
    let mut pump = Pump::new();
    pump.add_entry(property_entry("Stepper", prop.reader(), publisher));

    prop.set(&StepperVm {
        active: true,
        position: 12.5,
        state: StepperState::Running,
    });
    let stats = pump.tick();
    assert_eq!(stats.published, 1);

    let bytes = recv(&reader).expect("envelope delivered");
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["position"], 12.5);
    assert_eq!(json["active"], true);
    assert_eq!(json["state"], "Running");
}

#[test]
fn zero_subscriber_view_model_is_not_published_over_iox() {
    let node = make_node();
    let name = unique_name("zerosub");

    // No reader attached: subscriber_count is 0, so the non-exempt entry is
    // skipped (REQ_0862).
    let publisher = IoxVmPublisher::<N>::create(&node, &name).expect("create publisher");
    assert_eq!(publisher.subscriber_count(), 0);

    let prop = Property::<StepperVm>::new();
    let mut pump = Pump::new();
    pump.add_entry(property_entry("Stepper", prop.reader(), publisher));

    prop.set(&StepperVm {
        active: false,
        position: 1.0,
        state: StepperState::Idle,
    });
    let stats = pump.tick();
    assert_eq!(stats.published, 0);
    assert_eq!(stats.skipped_zero_sub, 1);
}

#[test]
fn late_joiner_receives_current_value_via_history_depth_one() {
    let node = make_node();
    let name = unique_name("late");

    let publisher = IoxVmPublisher::<N>::create(&node, &name).expect("create publisher");

    // Publish a value before any subscriber attaches. With history_size(1) the
    // service retains it.
    let early = StepperVm {
        active: true,
        position: 7.0,
        state: StepperState::Running,
    };
    publisher
        .publish(&serde_json::to_vec(&early).unwrap())
        .expect("publish early");

    // A UI starts up late and attaches.
    let factory = ServiceFactory::new(&node);
    let reader = factory
        .create_raw_reader_named::<N>(&name)
        .expect("late reader");

    // The connector's pump keeps publishing (here we publish once more, as the
    // next pump tick would). The late joiner receives a current value with no
    // resync handshake (REQ_0856 / REQ_0881).
    publisher
        .publish(&serde_json::to_vec(&early).unwrap())
        .expect("publish after late join");

    let bytes = recv(&reader).expect("late joiner received a value");
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["position"], 7.0);
}
