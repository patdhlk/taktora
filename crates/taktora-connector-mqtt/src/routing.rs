//! Per-channel routing struct (`MqttRouting`) and the topic / `MqttQos`
//! types it carries. `REQ_0251`, `REQ_0252`.

use taktora_connector_core::Routing;

use crate::topic::{MqttTopic, MqttTopicFilter};

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
    filter: Option<MqttTopicFilter>,
}

impl MqttRouting {
    /// Create a routing for `topic` at `qos` with `retained` cleared and
    /// no explicit inbound filter (a reader on this routing subscribes to
    /// the concrete `topic`).
    #[must_use]
    pub const fn new(topic: MqttTopic, qos: MqttQos) -> Self {
        Self {
            topic,
            qos,
            retained: false,
            filter: None,
        }
    }

    /// Builder-style setter for the retained-message flag (`REQ_0253`).
    #[must_use]
    pub const fn with_retained(mut self, retained: bool) -> Self {
        self.retained = retained;
        self
    }

    /// Builder-style setter for the inbound subscription filter
    /// (`REQ_0254`). When set, a reader created on this routing subscribes
    /// with `filter` — which may carry the MQTT wildcards `+` / `#` —
    /// instead of the concrete publish `topic`. Ignored on the outbound
    /// (publish) path.
    #[must_use]
    pub fn with_filter(mut self, filter: MqttTopicFilter) -> Self {
        self.filter = Some(filter);
        self
    }

    /// The inbound subscription filter for this routing (`REQ_0254`): the
    /// explicit [`Self::with_filter`] value if set, otherwise a filter
    /// derived from the concrete publish `topic` (a concrete topic is
    /// always a valid wildcard-free filter).
    ///
    /// # Panics
    ///
    /// Never in practice: a validated [`MqttTopic`] is a superset-valid
    /// [`MqttTopicFilter`], so the derived construction cannot fail.
    #[must_use]
    pub fn subscription_filter(&self) -> MqttTopicFilter {
        self.filter.clone().unwrap_or_else(|| {
            MqttTopicFilter::new(self.topic.as_str())
                .expect("a validated concrete topic is always a valid topic filter")
        })
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
        let topic = MqttTopic::new("taktora/examples/pubsub").unwrap();
        let r = MqttRouting::new(topic.clone(), MqttQos::AtLeastOnce);
        assert_eq!(r.topic(), &topic);
        assert_eq!(r.qos(), MqttQos::AtLeastOnce);
        assert!(!r.retained(), "retained defaults to false");

        let r = r.with_retained(true);
        assert!(r.retained());

        assert_eq!(MqttQos::AtMostOnce.wire_value(), 0);
        assert_eq!(MqttQos::AtLeastOnce.wire_value(), 1);
    }

    #[test]
    fn subscription_filter_derives_from_topic_or_uses_explicit() {
        // REQ_0254: with no explicit filter, the concrete topic doubles as
        // the (wildcard-free) subscription filter.
        let topic = MqttTopic::new("robot/arm/telemetry").unwrap();
        let r = MqttRouting::new(topic, MqttQos::AtLeastOnce);
        assert_eq!(r.subscription_filter().as_str(), "robot/arm/telemetry");

        // An explicit wildcard filter overrides the derived one.
        let r = r.with_filter(MqttTopicFilter::new("robot/+/telemetry").unwrap());
        assert_eq!(r.subscription_filter().as_str(), "robot/+/telemetry");
    }
}
