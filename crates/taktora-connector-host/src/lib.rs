//! `ConnectorHost` / `ConnectorGateway` builders, the framework's
//! [`Connector`] trait, and the [`HealthSubscription`] type returned by
//! `Connector::subscribe_health`. Implements
//! [`BB_0005`](../../spec/architecture/connector.rst) plus the
//! `Connector` trait itself (`REQ_0220`–`REQ_0224`).
//!
//! Layering:
//!
//! * `taktora-connector-core` defines the small types ([`Routing`],
//!   [`PayloadCodec`], [`ConnectorHealth`], [`HealthEvent`],
//!   [`ChannelDescriptor`], [`ConnectorError`]).
//! * `taktora-connector-transport-iox` defines the iceoryx2-backed
//!   handles ([`ChannelWriter`], [`ChannelReader`]).
//! * This crate ties them together via the [`Connector`] trait and
//!   composes them with a [`taktora_executor::Executor`].
//!
//! [`Routing`]: taktora_connector_core::Routing
//! [`PayloadCodec`]: taktora_connector_core::PayloadCodec
//! [`ConnectorHealth`]: taktora_connector_core::ConnectorHealth
//! [`HealthEvent`]: taktora_connector_core::HealthEvent
//! [`ChannelDescriptor`]: taktora_connector_core::ChannelDescriptor
//! [`ConnectorError`]: taktora_connector_core::ConnectorError
//! [`ChannelWriter`]: taktora_connector_transport_iox::ChannelWriter
//! [`ChannelReader`]: taktora_connector_transport_iox::ChannelReader

#![warn(missing_docs)]

pub mod connector;
pub mod gateway;
pub mod health_sub;
pub mod host;

pub use connector::Connector;
pub use gateway::{ConnectorGateway, ConnectorGatewayBuilder};
pub use health_sub::HealthSubscription;
pub use host::{ConnectorHost, ConnectorHostBuilder};
