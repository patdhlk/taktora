//! MQTT reference connector — M1 core (`BB_0036` / `FEAT_0036`).
//!
//! This is the protocol-agnostic core of the MQTT connector, unit-testable
//! against an in-process mock. It carries **no** real MQTT backend and **no**
//! `Connector` impl — those land in later milestones (M2a/M3).
//!
//! M1 modules:
//!
//! * [`routing`] — typed `MqttRouting`, `MqttTopic`, `MqttTopicFilter`,
//!   `MqttQos`; publish/filter validation (`REQ_0251`, `REQ_0252`).

#![warn(missing_docs)]

pub mod routing;

pub use routing::{MqttQos, MqttRouting, MqttTopic};
