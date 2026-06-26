//! [`UiConnectorOptions`]: configuration for the [`UiConnector`](crate::UiConnector)
//! (`REQ_0855`, `REQ_0872`, `REQ_0879`).
//!
//! Builder-style, mirroring `ZenohConnectorOptions`: a
//! `UiConnectorOptions::builder().build()` yields a working configuration with
//! sane defaults — the process-name instance namespace (reused from the manifest
//! module, never wall-clock), a ~30 Hz publish cadence, a tight command poll
//! interval, and bounded command / dedupe capacities.

use std::time::Duration;

use crate::manifest::default_instance;
use crate::system::default_epoch;

/// Default UI publish cadence: ~30 Hz. Fast enough for a responsive operator UI,
/// slow enough that the off-RT pump never competes with the control loop.
const DEFAULT_PUBLISH_CADENCE: Duration = Duration::from_millis(33);

/// Default command-handler poll interval. The handler is off the RT path, so a
/// few milliseconds of latency on accepting an invocation is imperceptible to a
/// human operator while keeping the poll loop cheap.
const DEFAULT_COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Default bound on a command's effect channel (`REQ_0871`).
const DEFAULT_COMMAND_CHANNEL_CAPACITY: usize = 16;

/// Default bound on the correlation-id dedupe LRU (`REQ_0867`).
const DEFAULT_DEDUPE_CAPACITY: usize = 256;

/// Process-wide configuration for the UI connector.
#[derive(Debug, Clone)]
pub struct UiConnectorOptions {
    /// The instance namespace prefixing every service name (`REQ_0873`).
    /// Defaults to the process name (see [`default_instance`]).
    pub instance: String,
    /// The process-unique epoch carried in the manifest and `SystemViewModel`
    /// heartbeat (`REQ_0879`, `REQ_0882`). Defaults to [`default_epoch`].
    pub epoch: u64,
    /// How often the non-RT pump publishes (`REQ_0861`). Default ~30 Hz.
    pub publish_cadence: Duration,
    /// How often the command handler polls for invocations. Default 5 ms.
    pub command_poll_interval: Duration,
    /// The bound on each command's effect channel (`REQ_0871`). Default 16.
    pub command_channel_capacity: usize,
    /// The bound on the correlation-id dedupe LRU (`REQ_0867`). Default 256.
    pub dedupe_capacity: usize,
}

impl UiConnectorOptions {
    /// Return a new [`UiConnectorOptionsBuilder`] with all defaults set.
    #[must_use]
    pub fn builder() -> UiConnectorOptionsBuilder {
        UiConnectorOptionsBuilder::default()
    }
}

impl Default for UiConnectorOptions {
    fn default() -> Self {
        Self::builder().build()
    }
}

/// Typed builder for [`UiConnectorOptions`].
#[derive(Debug, Clone)]
pub struct UiConnectorOptionsBuilder {
    instance: String,
    epoch: u64,
    publish_cadence: Duration,
    command_poll_interval: Duration,
    command_channel_capacity: usize,
    dedupe_capacity: usize,
}

impl Default for UiConnectorOptionsBuilder {
    fn default() -> Self {
        Self {
            instance: default_instance(),
            epoch: default_epoch(),
            publish_cadence: DEFAULT_PUBLISH_CADENCE,
            command_poll_interval: DEFAULT_COMMAND_POLL_INTERVAL,
            command_channel_capacity: DEFAULT_COMMAND_CHANNEL_CAPACITY,
            dedupe_capacity: DEFAULT_DEDUPE_CAPACITY,
        }
    }
}

impl UiConnectorOptionsBuilder {
    /// Override the instance namespace (`REQ_0873`).
    #[must_use]
    pub fn instance(mut self, instance: impl Into<String>) -> Self {
        self.instance = instance.into();
        self
    }

    /// Override the process epoch (`REQ_0879`).
    #[must_use]
    pub const fn epoch(mut self, epoch: u64) -> Self {
        self.epoch = epoch;
        self
    }

    /// Override the pump publish cadence (`REQ_0861`).
    #[must_use]
    pub const fn publish_cadence(mut self, cadence: Duration) -> Self {
        self.publish_cadence = cadence;
        self
    }

    /// Override the command-handler poll interval.
    #[must_use]
    pub const fn command_poll_interval(mut self, interval: Duration) -> Self {
        self.command_poll_interval = interval;
        self
    }

    /// Set each command's effect-channel capacity (clamped to at least 1).
    #[must_use]
    pub const fn command_channel_capacity(mut self, capacity: usize) -> Self {
        self.command_channel_capacity = if capacity == 0 { 1 } else { capacity };
        self
    }

    /// Set the correlation-id dedupe LRU capacity (clamped to at least 1).
    #[must_use]
    pub const fn dedupe_capacity(mut self, capacity: usize) -> Self {
        self.dedupe_capacity = if capacity == 0 { 1 } else { capacity };
        self
    }

    /// Consume the builder and return the final [`UiConnectorOptions`].
    #[must_use]
    pub fn build(self) -> UiConnectorOptions {
        UiConnectorOptions {
            instance: self.instance,
            epoch: self.epoch,
            publish_cadence: self.publish_cadence,
            command_poll_interval: self.command_poll_interval,
            command_channel_capacity: self.command_channel_capacity.max(1),
            dedupe_capacity: self.dedupe_capacity.max(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let o = UiConnectorOptions::builder().build();
        assert!(
            !o.instance.is_empty(),
            "instance defaults to the process name"
        );
        assert!(
            o.instance
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "instance must be a valid service-name component"
        );
        // ~30 Hz publish cadence by default.
        assert_eq!(o.publish_cadence, Duration::from_millis(33));
        assert_eq!(o.command_poll_interval, Duration::from_millis(5));
        assert_eq!(o.command_channel_capacity, 16);
        assert_eq!(o.dedupe_capacity, 256);
        assert_ne!(o.epoch, 0, "epoch defaults to the process epoch");
    }

    #[test]
    fn builder_overrides_apply() {
        let o = UiConnectorOptions::builder()
            .instance("my_app")
            .epoch(42)
            .publish_cadence(Duration::from_millis(16))
            .command_poll_interval(Duration::from_millis(1))
            .command_channel_capacity(64)
            .dedupe_capacity(1024)
            .build();
        assert_eq!(o.instance, "my_app");
        assert_eq!(o.epoch, 42);
        assert_eq!(o.publish_cadence, Duration::from_millis(16));
        assert_eq!(o.command_poll_interval, Duration::from_millis(1));
        assert_eq!(o.command_channel_capacity, 64);
        assert_eq!(o.dedupe_capacity, 1024);
    }

    #[test]
    fn capacities_are_clamped_to_at_least_one() {
        let o = UiConnectorOptions::builder()
            .command_channel_capacity(0)
            .dedupe_capacity(0)
            .build();
        assert_eq!(o.command_channel_capacity, 1);
        assert_eq!(o.dedupe_capacity, 1);
    }

    #[test]
    fn default_impl_matches_builder_default() {
        let a = UiConnectorOptions::default();
        let b = UiConnectorOptions::builder().build();
        assert_eq!(a.publish_cadence, b.publish_cadence);
        assert_eq!(a.command_channel_capacity, b.command_channel_capacity);
        assert_eq!(a.dedupe_capacity, b.dedupe_capacity);
    }
}
