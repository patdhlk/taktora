//! In-process [`MqttSessionLike`] implementation for M1 unit / integration
//! tests.
//!
//! [`MockMqttSession`] keeps a subscriber registry keyed by topic filter
//! and a log of every publish. A `publish` is dispatched synchronously to
//! every subscription whose filter matches the publish topic under the MQTT
//! wildcard rules ([`crate::matcher::topic_matches`]). The mock is **never**
//! feature-gated — it ships always so downstream test crates need no
//! protocol backend.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use crate::matcher::topic_matches;
use crate::routing::{MqttQos, MqttRouting};
use crate::session::{
    InboundRouter, MqttConnectionState, MqttSessionLike, PayloadSink, SessionError,
    SubscriptionHandle,
};
use crate::topic::{MqttTopic, MqttTopicFilter};

/// Shared-ownership sink so `publish` can clone the callbacks under the
/// lock and invoke them after releasing it.
type SharedSink = Arc<dyn Fn(&[u8]) + Send + Sync + 'static>;

struct SubscriberEntry {
    id: u64,
    filter: MqttTopicFilter,
    sink: SharedSink,
}

type SubscriberList = Arc<Mutex<Vec<SubscriberEntry>>>;

/// A single recorded publish: the topic string and the payload bytes.
pub type RecordedPublish = (String, Vec<u8>);

/// A single recorded publish with full routing detail — topic, payload,
/// QoS level, and the retained flag. M2a's outbound-path tests assert on
/// this to prove the QoS (`REQ_0252`) and retained flag (`REQ_0253`)
/// survive the dispatcher → `session.publish` boundary intact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishRecord {
    /// The publish topic string.
    pub topic: String,
    /// The payload bytes as delivered to `publish`.
    pub payload: Vec<u8>,
    /// The QoS the routing carried (`REQ_0252`).
    pub qos: MqttQos,
    /// The retained flag the routing carried (`REQ_0253`).
    pub retained: bool,
}

/// Shared record of broker UNSUBSCRIBE calls. Cloned into each
/// [`SubscriptionGuard`] so a dropped handle (the gateway sending
/// UNSUBSCRIBE when a filter's last channel is released, `REQ_0986`) is
/// observable via [`MockMqttSession::unsubscribe_calls`].
type CallLog = Arc<Mutex<Vec<String>>>;

/// In-process mock MQTT session. Round-trips publish → matching
/// subscription callbacks and records every publish.
///
/// M2b additions drive the inbound path deterministically: an installed
/// [`InboundRouter`] receives simulated inbound PUBLISHes via
/// [`Self::deliver_inbound`]; SUBSCRIBE / UNSUBSCRIBE calls are logged for
/// dedup / ref-count assertions (`REQ_0986`); and reconnect / CONNACK /
/// auth-reject transitions are simulated for the health mapping and
/// SUBSCRIBE-replay paths (`REQ_0980`–`REQ_0985`).
pub struct MockMqttSession {
    state: RwLock<MqttConnectionState>,
    subscribers: SubscriberList,
    next_sub_id: AtomicU64,
    published: Mutex<Vec<PublishRecord>>,
    inbound_router: Mutex<Option<InboundRouter>>,
    subscribe_log: Mutex<Vec<String>>,
    unsubscribe_log: CallLog,
    reconnect_attempts: AtomicU32,
}

impl std::fmt::Debug for MockMqttSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockMqttSession")
            .field("state", &self.state())
            .field("subscriber_count", &self.subscriber_count())
            .field("published_count", &self.published().len())
            .finish_non_exhaustive()
    }
}

impl Default for MockMqttSession {
    fn default() -> Self {
        Self::new()
    }
}

impl MockMqttSession {
    /// Create a fresh mock session starting in
    /// [`MqttConnectionState::Connected`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: RwLock::new(MqttConnectionState::Connected),
            subscribers: Arc::new(Mutex::new(Vec::new())),
            next_sub_id: AtomicU64::new(1),
            published: Mutex::new(Vec::new()),
            inbound_router: Mutex::new(None),
            subscribe_log: Mutex::new(Vec::new()),
            unsubscribe_log: Arc::new(Mutex::new(Vec::new())),
            reconnect_attempts: AtomicU32::new(0),
        }
    }

    /// Force the connection state. Tests use this to drive the health
    /// mapping.
    ///
    /// # Panics
    ///
    /// Panics if the internal state lock is poisoned.
    pub fn set_state(&self, state: MqttConnectionState) {
        *self.state.write().expect("mock state lock not poisoned") = state;
    }

    /// Number of live subscriptions.
    ///
    /// # Panics
    ///
    /// Panics if the subscriber lock is poisoned.
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.subscribers
            .lock()
            .expect("mock subscribers lock not poisoned")
            .len()
    }

    /// Snapshot of every recorded publish as `(topic, payload)`. Kept for
    /// M1 callers; M2a code wanting QoS / retained uses
    /// [`Self::published_detailed`].
    ///
    /// # Panics
    ///
    /// Panics if the published-log lock is poisoned.
    #[must_use]
    pub fn published(&self) -> Vec<RecordedPublish> {
        self.published
            .lock()
            .expect("mock published lock not poisoned")
            .iter()
            .map(|r| (r.topic.clone(), r.payload.clone()))
            .collect()
    }

    /// Snapshot of every recorded publish with full routing detail
    /// (topic, payload, QoS, retained). `REQ_0252`, `REQ_0253`.
    ///
    /// # Panics
    ///
    /// Panics if the published-log lock is poisoned.
    #[must_use]
    pub fn published_detailed(&self) -> Vec<PublishRecord> {
        self.published
            .lock()
            .expect("mock published lock not poisoned")
            .clone()
    }

    /// Simulate a broker-delivered inbound PUBLISH on the concrete
    /// `topic`. Invokes the installed [`InboundRouter`] exactly once so
    /// the gateway can run its local wildcard demux (`ADR_0129`,
    /// `REQ_0987`). A no-op if no router is installed.
    ///
    /// # Panics
    ///
    /// Panics if the router lock is poisoned.
    pub fn deliver_inbound(&self, topic: &MqttTopic, payload: &[u8]) {
        let router = self
            .inbound_router
            .lock()
            .expect("mock inbound router lock not poisoned")
            .clone();
        if let Some(router) = router {
            router(topic, payload);
        }
    }

    /// Snapshot the ordered log of SUBSCRIBE calls (one filter string per
    /// `subscribe`). Used to assert dedup + replay (`REQ_0986`,
    /// `REQ_0985`).
    ///
    /// # Panics
    ///
    /// Panics if the subscribe-log lock is poisoned.
    #[must_use]
    pub fn subscribe_calls(&self) -> Vec<String> {
        self.subscribe_log
            .lock()
            .expect("mock subscribe log lock not poisoned")
            .clone()
    }

    /// Snapshot the ordered log of UNSUBSCRIBE calls (one filter string
    /// per dropped subscription handle). Used to assert ref-counted
    /// teardown (`REQ_0986`).
    ///
    /// # Panics
    ///
    /// Panics if the unsubscribe-log lock is poisoned.
    #[must_use]
    pub fn unsubscribe_calls(&self) -> Vec<String> {
        self.unsubscribe_log
            .lock()
            .expect("mock unsubscribe log lock not poisoned")
            .clone()
    }

    /// Simulate the broker dropping the connection. State becomes
    /// [`MqttConnectionState::Disconnected`]; the reconnect-attempt count
    /// is unchanged (no attempt has been made yet).
    pub fn simulate_disconnect(&self, reason: impl Into<String>) {
        self.set_state(MqttConnectionState::Disconnected {
            reason: reason.into(),
        });
    }

    /// Simulate a failed reconnect attempt: bump the consecutive-attempt
    /// counter and enter [`MqttConnectionState::Connecting`] (`REQ_0983`).
    pub fn simulate_failed_reconnect(&self) {
        self.reconnect_attempts.fetch_add(1, Ordering::AcqRel);
        self.set_state(MqttConnectionState::Connecting);
    }

    /// Simulate a successful (re)connect CONNACK: clear the attempt
    /// counter and enter [`MqttConnectionState::Connected`]. The health
    /// watcher observes the transition into `Connected` and replays every
    /// active SUBSCRIBE (`REQ_0980`, `REQ_0985`).
    pub fn simulate_connack(&self) {
        self.reconnect_attempts.store(0, Ordering::Release);
        self.set_state(MqttConnectionState::Connected);
    }

    /// Simulate an authentication/authorization-rejected CONNACK. State
    /// becomes [`MqttConnectionState::AuthRejected`], mapping to a terminal
    /// `Down` (`REQ_0982`).
    pub fn simulate_auth_reject(&self, reason: impl Into<String>) {
        self.set_state(MqttConnectionState::AuthRejected {
            reason: reason.into(),
        });
    }
}

/// Drop guard inside [`SubscriptionHandle`]: removes the subscription from
/// the registry on drop and records the broker UNSUBSCRIBE (`REQ_0986`).
struct SubscriptionGuard {
    id: u64,
    filter: String,
    subscribers: SubscriberList,
    unsubscribe_log: CallLog,
}

impl Drop for SubscriptionGuard {
    fn drop(&mut self) {
        if let Ok(mut subs) = self.subscribers.lock() {
            subs.retain(|e| e.id != self.id);
        }
        if let Ok(mut log) = self.unsubscribe_log.lock() {
            log.push(self.filter.clone());
        }
    }
}

impl MqttSessionLike for MockMqttSession {
    fn state(&self) -> MqttConnectionState {
        self.state
            .read()
            .expect("mock state lock not poisoned")
            .clone()
    }

    fn reconnect_attempts(&self) -> u32 {
        self.reconnect_attempts.load(Ordering::Acquire)
    }

    // The trait returns `impl Future` because the real session awaits the
    // broker; the mock completes synchronously, so each method does its work
    // eagerly and hands back an already-resolved future.
    fn publish(
        &self,
        routing: &MqttRouting,
        payload: &[u8],
    ) -> impl std::future::Future<Output = Result<(), SessionError>> + Send {
        std::future::ready((|| -> Result<(), SessionError> {
            if !matches!(self.state(), MqttConnectionState::Connected) {
                return Err(SessionError::NotConnected {
                    reason: "mock session not connected".into(),
                });
            }
            let topic = routing.topic().clone();
            self.published
                .lock()
                .expect("mock published lock not poisoned")
                .push(PublishRecord {
                    topic: topic.as_str().to_owned(),
                    payload: payload.to_vec(),
                    qos: routing.qos(),
                    retained: routing.retained(),
                });

            // Snapshot the matching sinks under the lock, then invoke them
            // after releasing it (avoids holding the guard across callbacks).
            let sinks = self.matching_sinks(&topic);
            for sink in sinks {
                sink(payload);
            }
            Ok(())
        })())
    }

    fn subscribe(
        &self,
        filter: &MqttTopicFilter,
        sink: PayloadSink,
    ) -> impl std::future::Future<Output = Result<SubscriptionHandle, SessionError>> + Send {
        std::future::ready({
            let id = self.next_sub_id.fetch_add(1, Ordering::Relaxed);
            self.subscribe_log
                .lock()
                .expect("mock subscribe log lock not poisoned")
                .push(filter.as_str().to_owned());
            let entry = SubscriberEntry {
                id,
                filter: filter.clone(),
                sink: Arc::from(sink),
            };
            self.subscribers
                .lock()
                .expect("mock subscribers lock not poisoned")
                .push(entry);
            let guard = SubscriptionGuard {
                id,
                filter: filter.as_str().to_owned(),
                subscribers: Arc::clone(&self.subscribers),
                unsubscribe_log: Arc::clone(&self.unsubscribe_log),
            };
            Ok(SubscriptionHandle(Box::new(guard)))
        })
    }

    fn set_inbound_router(&self, router: InboundRouter) {
        *self
            .inbound_router
            .lock()
            .expect("mock inbound router lock not poisoned") = Some(router);
    }
}

impl MockMqttSession {
    /// Clone the sinks of every subscription whose filter matches `topic`.
    fn matching_sinks(&self, topic: &MqttTopic) -> Vec<SharedSink> {
        self.subscribers
            .lock()
            .expect("mock subscribers lock not poisoned")
            .iter()
            .filter(|e| topic_matches(&e.filter, topic))
            .map(|e| Arc::clone(&e.sink))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_mock_is_connected_and_empty() {
        let m = MockMqttSession::new();
        assert_eq!(m.state(), MqttConnectionState::Connected);
        assert_eq!(m.subscriber_count(), 0);
        assert!(m.published().is_empty());
    }

    #[test]
    fn set_state_round_trips() {
        let m = MockMqttSession::new();
        m.set_state(MqttConnectionState::Disconnected {
            reason: "bye".into(),
        });
        assert_eq!(
            m.state(),
            MqttConnectionState::Disconnected {
                reason: "bye".into()
            }
        );
    }

    #[test]
    fn matching_sinks_selects_by_filter() {
        // Pure (sync) check of the match-selection helper used by publish.
        let m = MockMqttSession::new();
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let h = std::sync::Arc::clone(&hits);
        {
            let mut subs = m.subscribers.lock().unwrap();
            subs.push(SubscriberEntry {
                id: 1,
                filter: MqttTopicFilter::new("a/+").unwrap(),
                sink: std::sync::Arc::new(move |_: &[u8]| {
                    h.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }),
            });
        }
        let matching = m.matching_sinks(&MqttTopic::new("a/b").unwrap());
        assert_eq!(matching.len(), 1);
        let none = m.matching_sinks(&MqttTopic::new("a/b/c").unwrap());
        assert!(none.is_empty());
    }
}
