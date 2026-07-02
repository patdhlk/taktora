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
//!   with subscriber fan-out + inbound-drop latch (`REQ_0261`).

#![warn(missing_docs)]

pub mod health;
pub mod matcher;
pub mod options;
pub mod routing;
pub mod topic;

pub use health::{MqttHealthError, MqttHealthMonitor};
pub use matcher::topic_matches;
pub use options::{Credentials, MqttConnectorOptions, MqttConnectorOptionsBuilder};
pub use routing::{MqttQos, MqttRouting};
pub use topic::{MqttTopic, MqttTopicFilter, TopicError, TopicFilterError};
