//! Per-channel routing registry for the MQTT gateway dispatcher.
//!
//! Mirrors `taktora_connector_zenoh::registry` in shape — the dispatcher
//! iterates this registry each tick to drive outbound traffic. MQTT's
//! direction model is binary (`Outbound` / `Inbound`), like Zenoh's.
//!
//! M2a only drives the `Outbound` bindings (`REQ_0252`, `REQ_0253`); the
//! `Inbound` binding is registered by `create_reader` so the service exists,
//! but its delivery is wired up in M2b (subscribe → fan-out).

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use taktora_connector_core::ConnectorError;

use crate::routing::MqttRouting;

/// Plugin-relative direction for one channel binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelDirection {
    /// Plugin → gateway → session. Plugin holds the iceoryx2 publisher
    /// (`ChannelWriter`); gateway holds the iceoryx2 subscriber
    /// (`RawChannelReader`) that drains into `session.publish`.
    Outbound,
    /// Session → gateway → plugin. Gateway holds the iceoryx2 publisher
    /// (`RawChannelWriter`); plugin holds the iceoryx2 subscriber
    /// (`ChannelReader`). Delivery is deferred to M2b.
    Inbound,
}

/// Gateway-side outbound drain — wraps an iceoryx2 raw subscriber so the
/// dispatcher can copy plugin-published bytes into a scratch buffer before
/// forwarding to `session.publish`.
///
/// `Send + Sync` so the dispatcher can hold the drain behind an
/// `Arc<dyn ...>` (the snapshot pattern in `drain_outbound_once`).
pub trait OutboundDrain: Send + Sync {
    /// Drain one envelope into `dest`. Implementations should be
    /// non-blocking — the dispatcher calls this in a tight loop. Returns
    /// `Ok(Some(n))` with the number of bytes copied; `Ok(None)` if no
    /// envelope was pending; `Err(...)` on failure.
    fn drain_into(&self, dest: &mut [u8]) -> Result<Option<usize>, ConnectorError>;
}

/// Gateway-side inbound publish — wraps an iceoryx2 raw publisher so
/// session callbacks can republish bytes verbatim on the channel's inbound
/// service. Registered by `create_reader`; driven by M2b.
pub trait InboundPublish: Send + Sync {
    /// Publish `bytes` verbatim. Implementations must be cheap because
    /// session callbacks may invoke this from hot paths.
    fn publish_bytes(&self, bytes: &[u8]) -> Result<(), ConnectorError>;
}

/// Channel ↔ iceoryx2 binding. Opaque to user code; the dispatcher matches
/// on the variant per tick.
///
/// The `Outbound` drain is held behind `Arc` so the async dispatcher can
/// snapshot-clone it out of the registry lock before awaiting on the
/// session (the lock-free iterate pattern).
pub enum ChannelBinding {
    /// Outbound — gateway drains bytes via the wrapped subscriber.
    Outbound(Arc<dyn OutboundDrain>),
    /// Inbound — gateway re-publishes bytes via the wrapped publisher
    /// (M2b drives this).
    Inbound(Box<dyn InboundPublish>),
}

impl fmt::Debug for ChannelBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Outbound(_) => f.write_str("Outbound(<dyn OutboundDrain>)"),
            Self::Inbound(_) => f.write_str("Inbound(<dyn InboundPublish>)"),
        }
    }
}

/// One entry in the [`ChannelRegistry`].
#[derive(Debug)]
pub struct RegisteredChannel {
    /// `ChannelDescriptor::name()` cloned at registration time. `Cow` so
    /// test fixtures can register `&'static` names without allocating.
    pub descriptor_name: Cow<'static, str>,
    /// The MQTT routing for this channel (topic, QoS, retained).
    pub routing: MqttRouting,
    /// Plugin-relative direction.
    pub direction: ChannelDirection,
    /// Source of bytes (outbound) or sink of bytes (inbound).
    pub binding: ChannelBinding,
}

/// Vec-backed channel registry.
///
/// Construct with [`ChannelRegistry::with_capacity`] for the expected
/// channel count; registrations beyond that capacity reallocate (acceptable
/// at startup, not on the dispatch hot path). Iteration is alloc-free.
#[derive(Debug, Default)]
pub struct ChannelRegistry {
    entries: Vec<RegisteredChannel>,
}

impl ChannelRegistry {
    /// Construct an empty registry pre-sized for `cap` channels.
    #[must_use]
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            entries: Vec::with_capacity(cap),
        }
    }

    /// Append a channel. Returns an error if a channel with the same
    /// `(name, direction)` tuple is already registered.
    ///
    /// # Errors
    ///
    /// [`ConnectorError::Configuration`] on a duplicate `(name, direction)`.
    pub fn register(
        &mut self,
        name: String,
        routing: MqttRouting,
        direction: ChannelDirection,
        binding: ChannelBinding,
    ) -> Result<(), ConnectorError> {
        let duplicate = self
            .entries
            .iter()
            .any(|e| e.descriptor_name == name && e.direction == direction);
        if duplicate {
            return Err(ConnectorError::Configuration(format!(
                "channel '{name}' already registered with direction {direction:?}",
            )));
        }
        self.entries.push(RegisteredChannel {
            descriptor_name: Cow::Owned(name),
            routing,
            direction,
            binding,
        });
        Ok(())
    }

    /// Number of registered channels.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when no channels have been registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate channels in registration order. Allocation-free.
    pub fn iter(&self) -> std::slice::Iter<'_, RegisteredChannel> {
        self.entries.iter()
    }
}

impl<'a> IntoIterator for &'a ChannelRegistry {
    type Item = &'a RegisteredChannel;
    type IntoIter = std::slice::Iter<'a, RegisteredChannel>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::MqttQos;
    use crate::topic::MqttTopic;

    struct NullDrain;
    impl OutboundDrain for NullDrain {
        fn drain_into(&self, _dest: &mut [u8]) -> Result<Option<usize>, ConnectorError> {
            Ok(None)
        }
    }

    fn routing(topic: &str) -> MqttRouting {
        MqttRouting::new(MqttTopic::new(topic).unwrap(), MqttQos::AtMostOnce)
    }

    #[test]
    fn register_rejects_duplicate_name_direction() {
        let mut reg = ChannelRegistry::with_capacity(2);
        reg.register(
            "a".into(),
            routing("t/a"),
            ChannelDirection::Outbound,
            ChannelBinding::Outbound(Arc::new(NullDrain)),
        )
        .expect("first register");
        let err = reg
            .register(
                "a".into(),
                routing("t/a"),
                ChannelDirection::Outbound,
                ChannelBinding::Outbound(Arc::new(NullDrain)),
            )
            .expect_err("duplicate rejected");
        assert!(matches!(err, ConnectorError::Configuration(_)));
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn iter_is_registration_order() {
        let mut reg = ChannelRegistry::with_capacity(2);
        for name in ["x", "y"] {
            reg.register(
                name.into(),
                routing("t/x"),
                ChannelDirection::Outbound,
                ChannelBinding::Outbound(Arc::new(NullDrain)),
            )
            .unwrap();
        }
        let names: Vec<&str> = reg.iter().map(|e| e.descriptor_name.as_ref()).collect();
        assert_eq!(names, ["x", "y"]);
        assert!(!reg.is_empty());
    }
}
