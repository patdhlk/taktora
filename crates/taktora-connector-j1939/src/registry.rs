//! Per-channel routing registry used by the gateway's RX/TX loops.
//! `BB_0100`.
//!
//! Maps each registered channel to its [`J1939Routing`] and direction
//! so the dispatcher can iterate matching channels on every RX frame
//! (PGN/SA/DA demux) and per TX drain.
//!
//! Unlike `taktora_connector_can::ChannelRegistry`, J1939 routing
//! carries no interface — a PGN names a logical message on the bus, not
//! a wire. For this tracer bullet the registry is therefore **not**
//! keyed by interface: each per-iface dispatcher iterates every channel
//! of the relevant direction and matches purely on PGN / SA / DA. With
//! a single configured interface (the common case and every test here)
//! this is exactly right. Interface-scoped channels are a documented
//! seam for #126 (address-claim is inherently per-interface) — add an
//! optional `iface` field here and filter in `iter_direction`.
//!
//! The [`ChannelBinding`], [`Direction`], `OutboundDrain`, and
//! `InboundPublish` types are reused verbatim from
//! `taktora-connector-can` (`REQ_0899`) — they are protocol-neutral
//! iceoryx2 plumbing.

use std::borrow::Cow;
use std::fmt;

use taktora_connector_can::{ChannelBinding, Direction};

use crate::routing::J1939Routing;

/// One entry in the [`J1939Registry`].
pub struct RegisteredChannel {
    /// `ChannelDescriptor::name()` cloned at registration time.
    pub descriptor_name: Cow<'static, str>,
    /// The J1939 routing this channel's frames map to.
    pub routing: J1939Routing,
    /// Outbound (writer) or inbound (reader).
    pub direction: Direction,
    /// Source of bytes (outbound) or sink of bytes (inbound).
    pub binding: ChannelBinding,
}

impl fmt::Debug for RegisteredChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegisteredChannel")
            .field("descriptor_name", &self.descriptor_name)
            .field("routing", &self.routing)
            .field("direction", &self.direction)
            .finish_non_exhaustive()
    }
}

/// Opaque handle returned from [`J1939Registry::register`].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ChannelHandle(pub usize);

/// Vec-backed registry. Iteration is stable in insertion order and
/// allocation-free.
#[derive(Debug, Default)]
pub struct J1939Registry {
    channels: Vec<RegisteredChannel>,
}

impl J1939Registry {
    /// Construct an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct with pre-allocated capacity. `register` is alloc-free
    /// until `capacity` is exceeded.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            channels: Vec::with_capacity(capacity),
        }
    }

    /// Append a channel and return its handle.
    pub fn register(
        &mut self,
        descriptor_name: impl Into<Cow<'static, str>>,
        routing: J1939Routing,
        direction: Direction,
        binding: ChannelBinding,
    ) -> ChannelHandle {
        let handle = ChannelHandle(self.channels.len());
        self.channels.push(RegisteredChannel {
            descriptor_name: descriptor_name.into(),
            routing,
            direction,
            binding,
        });
        handle
    }

    /// Number of registered channels.
    #[must_use]
    pub fn len(&self) -> usize {
        self.channels.len()
    }

    /// `true` when no channels have been registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.channels.is_empty()
    }

    /// Iterate channels in registration order. Alloc-free.
    pub fn iter(&self) -> std::slice::Iter<'_, RegisteredChannel> {
        self.channels.iter()
    }

    /// Iterate channels of the given direction, in registration order.
    /// Alloc-free.
    pub fn iter_direction(&self, direction: Direction) -> impl Iterator<Item = &RegisteredChannel> {
        self.channels
            .iter()
            .filter(move |c| c.direction == direction)
    }

    /// Borrow a single channel by handle.
    #[must_use]
    pub fn get(&self, handle: ChannelHandle) -> Option<&RegisteredChannel> {
        self.channels.get(handle.0)
    }
}

impl<'a> IntoIterator for &'a J1939Registry {
    type Item = &'a RegisteredChannel;
    type IntoIter = std::slice::Iter<'a, RegisteredChannel>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::Pgn;

    #[test]
    fn iter_direction_filters_and_preserves_order() {
        let mut r = J1939Registry::with_capacity(3);
        r.register(
            "in0",
            J1939Routing::single_frame(Pgn::new(59904).unwrap()),
            Direction::Inbound,
            ChannelBinding::Unbound,
        );
        r.register(
            "out0",
            J1939Routing::single_frame(Pgn::new(60928).unwrap()),
            Direction::Outbound,
            ChannelBinding::Unbound,
        );
        r.register(
            "in1",
            J1939Routing::single_frame(Pgn::new(65270).unwrap()),
            Direction::Inbound,
            ChannelBinding::Unbound,
        );

        let inbound: Vec<&str> = r
            .iter_direction(Direction::Inbound)
            .map(|c| c.descriptor_name.as_ref())
            .collect();
        assert_eq!(inbound, vec!["in0", "in1"]);

        let outbound: Vec<&str> = r
            .iter_direction(Direction::Outbound)
            .map(|c| c.descriptor_name.as_ref())
            .collect();
        assert_eq!(outbound, vec!["out0"]);
    }
}
