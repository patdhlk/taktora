//! Gateway-side inbound demux + reference-counted subscription table
//! (`ADR_0129`, `ADR_0130`; `REQ_0254`, `REQ_0987`, `REQ_0986`,
//! `REQ_0985`).
//!
//! MQTT delivers each inbound PUBLISH tagged with its *concrete* topic,
//! not with the subscription that matched, so the connector decides for
//! itself which [`ChannelReader`](taktora_connector_transport_iox::ChannelReader)
//! instances a message belongs to. This module owns that decision:
//!
//! * [`InboundTable::routes`] — one entry per inbound channel (its filter
//!   plus the gateway-side publisher that forwards matched bytes to the
//!   plugin). On each PUBLISH the gateway matches the concrete topic
//!   locally against *every* route's filter and fans out to all matches
//!   (`REQ_0987`).
//! * [`InboundTable::subscriptions`] — one entry per **distinct** filter,
//!   reference-counted over the channels using it. The broker sees a
//!   filter SUBSCRIBE at most once, and an UNSUBSCRIBE only when the last
//!   channel referencing it is released (`REQ_0986`).
//! * [`InboundTable::replay_handles`] — SUBSCRIBE handles minted by the
//!   reconnect replay path, held for the connector's lifetime so the
//!   replay does not immediately UNSUBSCRIBE (`REQ_0985`).

use std::sync::Arc;
use std::sync::Mutex;

use crate::matcher::topic_matches;
use crate::registry::InboundPublish;
use crate::session::SubscriptionHandle;
use crate::topic::{MqttTopic, MqttTopicFilter};

/// One inbound channel's demux route: its subscription filter and the
/// gateway-side publisher that forwards matched bytes to the plugin.
struct InboundRoute {
    filter: MqttTopicFilter,
    publisher: Arc<dyn InboundPublish>,
    descriptor_name: String,
}

/// One distinct broker subscription: the filter, the number of channels
/// referencing it, and the session handle whose drop sends UNSUBSCRIBE.
struct BrokerSubscription {
    filter: MqttTopicFilter,
    refcount: usize,
    handle: SubscriptionHandle,
}

/// Gateway-side inbound routing + subscription state. Shared behind an
/// `Arc<Mutex<..>>` between the connector (channel setup), the installed
/// [`crate::session::InboundRouter`] (fan-out), and the health watcher
/// (reconnect replay).
#[derive(Default)]
pub struct InboundTable {
    routes: Vec<InboundRoute>,
    subscriptions: Vec<BrokerSubscription>,
    replay_handles: Vec<SubscriptionHandle>,
}

impl std::fmt::Debug for InboundTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InboundTable")
            .field("route_count", &self.routes.len())
            .field("distinct_filters", &self.subscriptions.len())
            .finish_non_exhaustive()
    }
}

impl InboundTable {
    /// Construct an empty table.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            routes: Vec::new(),
            subscriptions: Vec::new(),
            replay_handles: Vec::new(),
        }
    }

    /// Register an inbound channel's demux route and bump the filter's
    /// reference count. Returns `true` when this is the **first** channel
    /// for `filter`, i.e. the caller must send a broker SUBSCRIBE and then
    /// record the handle via [`Self::record_subscription`] (`REQ_0986`).
    pub fn add_route(
        &mut self,
        filter: MqttTopicFilter,
        publisher: Arc<dyn InboundPublish>,
        descriptor_name: String,
    ) -> bool {
        self.routes.push(InboundRoute {
            filter: filter.clone(),
            publisher,
            descriptor_name,
        });
        match self.subscriptions.iter_mut().find(|s| s.filter == filter) {
            Some(existing) => {
                existing.refcount += 1;
                false
            }
            None => true,
        }
    }

    /// Record the broker SUBSCRIBE handle for a filter's first channel
    /// (ref-count 1). Paired with an [`Self::add_route`] that returned
    /// `true`.
    pub fn record_subscription(&mut self, filter: MqttTopicFilter, handle: SubscriptionHandle) {
        self.subscriptions.push(BrokerSubscription {
            filter,
            refcount: 1,
            handle,
        });
    }

    /// Release the inbound channel named `descriptor_name`, decrementing
    /// its filter's reference count. When the count reaches zero the
    /// [`BrokerSubscription`] is dropped, sending UNSUBSCRIBE (`REQ_0986`).
    /// No-op if the channel is not registered.
    pub fn remove_channel(&mut self, descriptor_name: &str) {
        let Some(pos) = self
            .routes
            .iter()
            .position(|r| r.descriptor_name == descriptor_name)
        else {
            return;
        };
        let filter = self.routes.remove(pos).filter;
        if let Some(spos) = self.subscriptions.iter().position(|s| s.filter == filter) {
            self.subscriptions[spos].refcount -= 1;
            if self.subscriptions[spos].refcount == 0 {
                // Explicitly drop the SUBSCRIBE handle → the session sends
                // UNSUBSCRIBE (`REQ_0986`). Moving `handle` out is also its
                // only read, keeping the field live.
                let released = self.subscriptions.remove(spos);
                drop(released.handle);
            }
        }
    }

    /// The distinct active filters, one per broker subscription. Used by
    /// the reconnect replay path (`REQ_0985`).
    #[must_use]
    pub fn active_filters(&self) -> Vec<MqttTopicFilter> {
        self.subscriptions.iter().map(|s| s.filter.clone()).collect()
    }

    /// Hold a replay-minted SUBSCRIBE handle for the connector's lifetime
    /// so the replay does not immediately UNSUBSCRIBE (`REQ_0985`).
    pub fn push_replay_handle(&mut self, handle: SubscriptionHandle) {
        self.replay_handles.push(handle);
    }

    /// Number of distinct broker subscriptions (test / introspection).
    #[must_use]
    pub fn distinct_filter_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Snapshot the publishers whose route filter matches `topic`. Cloned
    /// under the lock so callers fan out after releasing it.
    fn matching_publishers(&self, topic: &MqttTopic) -> Vec<Arc<dyn InboundPublish>> {
        self.routes
            .iter()
            .filter(|r| topic_matches(&r.filter, topic))
            .map(|r| Arc::clone(&r.publisher))
            .collect()
    }
}

/// Run the gateway-local wildcard demux for one inbound PUBLISH: match
/// `topic` against every registered channel filter and forward `payload`
/// to each matching channel's gateway-side publisher (`ADR_0129`,
/// `REQ_0987`).
///
/// The matching set is snapshotted under the lock, then published to
/// after the lock is released, so a slow / saturating publisher never
/// holds the table mutex.
pub fn route_inbound(table: &Arc<Mutex<InboundTable>>, topic: &MqttTopic, payload: &[u8]) {
    let publishers = {
        let guard = table.lock().expect("inbound table mutex not poisoned");
        guard.matching_publishers(topic)
    };
    for publisher in publishers {
        let _ = publisher.publish_bytes(payload);
    }
}
