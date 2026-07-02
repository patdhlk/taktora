//! Per-channel routing struct (`MqttRouting`) and the topic / `MqttQos`
//! types it carries. `REQ_0251`, `REQ_0252`.

use taktora_connector_core::Routing;

/// MQTT Quality-of-Service level. Only QoS 0 and 1 are supported; QoS 2 is
/// deferred to a follow-on spec (`REQ_0252`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MqttQos {
    /// QoS 0 — fire-and-forget, at most one delivery.
    AtMostOnce,
    /// QoS 1 — acknowledged delivery, at least one delivery.
    AtLeastOnce,
}

impl MqttQos {
    /// The MQTT wire value for this QoS level (0 or 1).
    #[must_use]
    pub const fn wire_value(self) -> u8 {
        match self {
            Self::AtMostOnce => 0,
            Self::AtLeastOnce => 1,
        }
    }
}

/// Per-channel routing for the MQTT connector. Carries the publish topic,
/// the QoS level, and the retained-message flag. Implements the [`Routing`]
/// marker (`REQ_0251`): `Clone + Send + Sync + Debug + 'static`, no methods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MqttRouting {
    topic: MqttTopic,
    qos: MqttQos,
    retained: bool,
}

impl MqttRouting {
    /// Create a routing for `topic` at `qos` with `retained` cleared.
    #[must_use]
    pub const fn new(topic: MqttTopic, qos: MqttQos) -> Self {
        Self {
            topic,
            qos,
            retained: false,
        }
    }

    /// Builder-style setter for the retained-message flag (`REQ_0253`).
    #[must_use]
    pub const fn with_retained(mut self, retained: bool) -> Self {
        self.retained = retained;
        self
    }

    /// Borrow the publish topic.
    #[must_use]
    pub const fn topic(&self) -> &MqttTopic {
        &self.topic
    }

    /// Return the QoS level.
    #[must_use]
    pub const fn qos(&self) -> MqttQos {
        self.qos
    }

    /// Return the retained-message flag.
    #[must_use]
    pub const fn retained(&self) -> bool {
        self.retained
    }
}

impl Routing for MqttRouting {}

/// A validated MQTT **publish** topic name. Placeholder for slice 2 — the
/// full validator lands there.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MqttTopic(String);

impl MqttTopic {
    /// Construct without validation (temporary — slice 2 replaces this).
    #[must_use]
    pub fn new_unchecked(topic: impl Into<String>) -> Self {
        Self(topic.into())
    }

    /// Borrow the topic string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_routing_bounds<T: Clone + Send + Sync + std::fmt::Debug + 'static>() {}

    #[test]
    fn mqtt_routing_impls_routing_marker_bounds() {
        // REQ_0251: MqttRouting implements the Routing marker trait, i.e. it
        // satisfies `Clone + Send + Sync + Debug + 'static`.
        assert_routing_bounds::<MqttRouting>();
        fn takes_routing<R: Routing>() {}
        takes_routing::<MqttRouting>();
    }

    #[test]
    fn mqtt_routing_carries_topic_qos_retained() {
        // REQ_0251: carries topic + qos + retained; REQ_0252: QoS 0 and 1.
        let topic = MqttTopic::new_unchecked("taktora/examples/pubsub");
        let r = MqttRouting::new(topic.clone(), MqttQos::AtLeastOnce);
        assert_eq!(r.topic(), &topic);
        assert_eq!(r.qos(), MqttQos::AtLeastOnce);
        assert!(!r.retained(), "retained defaults to false");

        let r = r.with_retained(true);
        assert!(r.retained());

        assert_eq!(MqttQos::AtMostOnce.wire_value(), 0);
        assert_eq!(MqttQos::AtLeastOnce.wire_value(), 1);
    }
}
