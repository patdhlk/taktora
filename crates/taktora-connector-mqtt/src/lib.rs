//! MQTT reference connector — M1 core (`BB_0036` / `FEAT_0036`).
//!
//! This is the protocol-agnostic core of the MQTT connector, unit-testable
//! against an in-process mock. It carries **no** real MQTT backend and **no**
//! `Connector` impl — those land in later milestones (M2a/M3).
//!
//! M1 modules:
//!
//! * [`topic`] — `MqttTopic` / `MqttTopicFilter` with publish/filter
//!   validation (`REQ_0251`, `REQ_0254`).
//! * [`matcher`] — local filter-vs-topic matcher (`REQ_0254`,
//!   groundwork for the M2b demux, `ADR_0129`).
//! * [`routing`] — typed `MqttRouting`, `MqttQos` (`REQ_0251`, `REQ_0252`).
//! * [`options`] — `MqttConnectorOptions` typed builder with bounded
//!   bridge capacities (`REQ_0259`).
//! * [`health`] — `MqttHealthMonitor` reusing the core `HealthMonitor`
//!   with subscriber fan-out + inbound-drop latch (`REQ_0261`) and the
//!   outbound-backpressure latch (`REQ_0260`).
//! * [`session`] — the async `MqttSessionLike` back-end seam.
//! * [`mock`] — in-process `MockMqttSession` (always built).
//! * [`bridge`] — bounded `OutboundBridge` / `InboundBridge` with
//!   drop-accounting (`REQ_0259`, `REQ_0260`, `REQ_0261`).
//!
//! M2a modules (outbound path):
//!
//! * [`registry`] — per-channel `ChannelRegistry` + `Outbound` / `Inbound`
//!   bindings the dispatcher iterates.
//! * [`gateway`] — `MqttGateway`, the crate-contained tokio sidecar
//!   (`REQ_0258`).
//! * [`dispatcher`] — outbound-drain loop calling `session.publish` with the
//!   routing's QoS / retained flag (`REQ_0252`, `REQ_0253`), plus the
//!   `BridgedOutbound` saturation gate (`REQ_0260`).
//! * [`connector`] — `MqttConnector<C>` implementing `Connector`
//!   (`REQ_0250`).

#![warn(missing_docs)]
// Allow MQTT domain identifiers (QoS, MQTT, CONNACK, CONNECT, PUBLISH,
// SUBSCRIBE, …) to appear in docstrings without backticks. Matches the
// posture the CAN and EtherCAT connector crates take for their fieldbus
// terminology.
#![allow(clippy::doc_markdown)]

pub mod bridge;
pub mod connector;
pub mod dispatcher;
pub mod gateway;
pub mod health;
pub mod inbound;
pub mod matcher;
pub mod mock;
pub mod options;
pub mod registry;
pub mod routing;
pub mod session;
pub mod topic;

pub use bridge::{InboundBridge, InboundOutcome, OutboundBridge, OutboundError};
pub use connector::{MqttConnector, MqttState};
pub use dispatcher::{
    BridgedInboundPublish, BridgedOutbound, DEFAULT_DISPATCHER_TICK, IoxInboundPublish,
    IoxOutboundDrain, dispatch_outbound_once, dispatcher_loop,
};
pub use gateway::MqttGateway;
pub use health::{MqttHealthError, MqttHealthMonitor};
pub use inbound::{InboundTable, route_inbound};
pub use matcher::topic_matches;
pub use mock::{MockMqttSession, PublishRecord, RecordedPublish};
pub use options::{Credentials, MqttConnectorOptions, MqttConnectorOptionsBuilder, TlsOptions};
pub use registry::{
    ChannelBinding, ChannelDirection, ChannelRegistry, InboundPublish, OutboundDrain,
    RegisteredChannel,
};
pub use routing::{MqttQos, MqttRouting};
pub use session::{
    InboundRouter, MqttConnectionState, MqttSessionLike, PayloadSink, SessionError,
    SubscriptionHandle,
};
pub use topic::{MqttTopic, MqttTopicFilter, TopicError, TopicFilterError};
