//! In-process [`MqttSessionLike`] implementation for M1 unit / integration
//! tests.
//!
//! [`MockMqttSession`] keeps a subscriber registry keyed by topic filter
//! and a log of every publish. A `publish` is dispatched synchronously to
//! every subscription whose filter matches the publish topic under the MQTT
//! wildcard rules ([`crate::matcher::topic_matches`]). The mock is **never**
//! feature-gated — it ships always so downstream test crates need no
//! protocol backend.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use crate::matcher::topic_matches;
use crate::routing::MqttRouting;
use crate::session::{
    MqttConnectionState, MqttSessionLike, PayloadSink, SessionError, SubscriptionHandle,
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

/// In-process mock MQTT session. Round-trips publish → matching
/// subscription callbacks and records every publish.
pub struct MockMqttSession {
    state: RwLock<MqttConnectionState>,
    subscribers: SubscriberList,
    next_sub_id: AtomicU64,
    published: Mutex<Vec<RecordedPublish>>,
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

    /// Snapshot of every recorded publish (topic, payload).
    ///
    /// # Panics
    ///
    /// Panics if the published-log lock is poisoned.
    #[must_use]
    pub fn published(&self) -> Vec<RecordedPublish> {
        self.published
            .lock()
            .expect("mock published lock not poisoned")
            .clone()
    }
}

/// Drop guard inside [`SubscriptionHandle`]: removes the subscription from
/// the registry on drop.
struct SubscriptionGuard {
    id: u64,
    subscribers: SubscriberList,
}

impl Drop for SubscriptionGuard {
    fn drop(&mut self) {
        if let Ok(mut subs) = self.subscribers.lock() {
            subs.retain(|e| e.id != self.id);
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

    async fn publish(&self, routing: &MqttRouting, payload: &[u8]) -> Result<(), SessionError> {
        if !matches!(self.state(), MqttConnectionState::Connected) {
            return Err(SessionError::NotConnected {
                reason: "mock session not connected".into(),
            });
        }
        let topic = routing.topic().clone();
        self.published
            .lock()
            .expect("mock published lock not poisoned")
            .push((topic.as_str().to_owned(), payload.to_vec()));

        // Snapshot the matching sinks under the lock, then invoke them
        // after releasing it (avoids holding the guard across callbacks).
        let sinks = self.matching_sinks(&topic);
        for sink in sinks {
            sink(payload);
        }
        Ok(())
    }

    async fn subscribe(
        &self,
        filter: &MqttTopicFilter,
        sink: PayloadSink,
    ) -> Result<SubscriptionHandle, SessionError> {
        let id = self.next_sub_id.fetch_add(1, Ordering::Relaxed);
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
            subscribers: Arc::clone(&self.subscribers),
        };
        Ok(SubscriptionHandle(Box::new(guard)))
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
    use crate::MqttQos;

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
        let _ = MqttQos::AtMostOnce; // keep the import meaningful
    }
}
