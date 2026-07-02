//! Bounded bridges between the taktora-executor side (plugin) and the tokio
//! sidecar (gateway). `REQ_0259`, `REQ_0260`, `REQ_0261`.
//!
//! * [`OutboundBridge`] — plugin → gateway. Saturation surfaces as
//!   [`OutboundError::BackPressure`] (`REQ_0260`).
//! * [`InboundBridge`] — gateway → plugin. Saturation drops the message and
//!   returns [`InboundOutcome::Dropped { count }`] carrying the running
//!   cumulative drop count, which the gateway folds into
//!   [`crate::MqttHealthMonitor::record_inbound_drop`] (`REQ_0261`).
//!
//! Shape is intentionally identical to
//! `taktora_connector_zenoh::bridge` — the framework-level health-emission
//! logic stays uniform across connectors.

use std::sync::atomic::{AtomicU64, Ordering};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};

/// Plugin → gateway bridge. Bounded capacity is fixed at construction.
#[derive(Debug)]
pub struct OutboundBridge<T> {
    tx: Sender<T>,
    rx: Receiver<T>,
    capacity: usize,
}

/// Errors surfaced from [`OutboundBridge::try_send`].
#[derive(Debug, thiserror::Error)]
pub enum OutboundError<T> {
    /// Channel is full — the message was rejected; the caller surfaces
    /// back-pressure to the application (`REQ_0260`).
    #[error("outbound bridge full (capacity exceeded)")]
    BackPressure(T),
    /// All receivers have been dropped — the gateway is gone.
    #[error("outbound bridge disconnected")]
    Disconnected(T),
}

impl<T> OutboundError<T> {
    /// Recover the message that failed to send.
    pub fn into_inner(self) -> T {
        match self {
            Self::BackPressure(t) | Self::Disconnected(t) => t,
        }
    }
}

impl<T> OutboundBridge<T> {
    /// Construct a bridge with the given bounded capacity (clamped to at
    /// least 1).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(1);
        let (tx, rx) = bounded(cap);
        Self {
            tx,
            rx,
            capacity: cap,
        }
    }

    /// Try to send a message without blocking.
    ///
    /// # Errors
    ///
    /// [`OutboundError::BackPressure`] when the channel is full;
    /// [`OutboundError::Disconnected`] when the receiver is gone.
    pub fn try_send(&self, msg: T) -> Result<(), OutboundError<T>> {
        match self.tx.try_send(msg) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(t)) => Err(OutboundError::BackPressure(t)),
            Err(TrySendError::Disconnected(t)) => Err(OutboundError::Disconnected(t)),
        }
    }

    /// Receive the next message, blocking until one is available or the
    /// channel disconnects. Used on the gateway side.
    pub fn recv(&self) -> Option<T> {
        self.rx.recv().ok()
    }

    /// Try to receive without blocking.
    pub fn try_recv(&self) -> Option<T> {
        self.rx.try_recv().ok()
    }

    /// The channel's bounded capacity.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }
}

/// Gateway → plugin bridge. On overflow the message is dropped and a running
/// dropped-count is incremented (`REQ_0261`).
#[derive(Debug)]
pub struct InboundBridge<T> {
    tx: Sender<T>,
    rx: Receiver<T>,
    capacity: usize,
    dropped: AtomicU64,
}

/// Outcome of [`InboundBridge::try_send`].
#[derive(Debug, PartialEq, Eq)]
pub enum InboundOutcome {
    /// The message was enqueued.
    Sent,
    /// The channel was full — the message was dropped and the caller is
    /// given the running cumulative drop-count (`REQ_0261`).
    Dropped {
        /// Cumulative count of inbound messages dropped on this bridge
        /// since construction.
        count: u64,
    },
}

impl<T> InboundBridge<T> {
    /// Construct a bridge with the given bounded capacity (clamped to at
    /// least 1).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(1);
        let (tx, rx) = bounded(cap);
        Self {
            tx,
            rx,
            capacity: cap,
            dropped: AtomicU64::new(0),
        }
    }

    /// Try to enqueue an inbound message. On full, the message is dropped
    /// and the dropped-count is incremented (`REQ_0261`).
    pub fn try_send(&self, msg: T) -> InboundOutcome {
        match self.tx.try_send(msg) {
            Ok(()) => InboundOutcome::Sent,
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                let count = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                InboundOutcome::Dropped { count }
            }
        }
    }

    /// Try to receive without blocking.
    pub fn try_recv(&self) -> Option<T> {
        self.rx.try_recv().ok()
    }

    /// The channel's bounded capacity.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Cumulative count of inbound drops since construction.
    #[must_use]
    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbound_backpressure_on_full() {
        // REQ_0259/REQ_0260: bounded; a full channel yields BackPressure.
        let b = OutboundBridge::<u32>::new(2);
        assert_eq!(b.capacity(), 2);
        assert!(b.try_send(1).is_ok());
        assert!(b.try_send(2).is_ok());
        match b.try_send(3) {
            Err(OutboundError::BackPressure(v)) => assert_eq!(v, 3),
            other => panic!("expected BackPressure, got {other:?}"),
        }
        // Draining one frees a slot again.
        assert_eq!(b.try_recv(), Some(1));
        assert!(b.try_send(3).is_ok());
    }

    #[test]
    fn outbound_capacity_clamped_to_one() {
        let b = OutboundBridge::<u8>::new(0);
        assert_eq!(b.capacity(), 1);
    }

    #[test]
    fn inbound_drops_and_counts_past_capacity() {
        // REQ_0261: past capacity, messages are dropped and counted.
        let b = InboundBridge::<u32>::new(2);
        assert_eq!(b.try_send(1), InboundOutcome::Sent);
        assert_eq!(b.try_send(2), InboundOutcome::Sent);
        assert_eq!(b.try_send(3), InboundOutcome::Dropped { count: 1 });
        assert_eq!(b.try_send(4), InboundOutcome::Dropped { count: 2 });
        assert_eq!(b.dropped_count(), 2);
        // The two that fit are still receivable in order.
        assert_eq!(b.try_recv(), Some(1));
        assert_eq!(b.try_recv(), Some(2));
        assert_eq!(b.try_recv(), None);
    }
}
