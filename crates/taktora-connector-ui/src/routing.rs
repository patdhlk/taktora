//! [`UiRouting`] — the routing type for the UI connector (REQ_0857).

use taktora_connector_core::routing::Routing;
use taktora_connector_ui_contract::Kind;

/// The UI connector's routing key: a logical entry `name` plus its [`Kind`].
///
/// Every UI connector channel — a Property ViewModel, a Command request/reply,
/// a CanExecute gate, the manifest, or the system heartbeat — is addressed by
/// its name and kind. The connector resolves these into instance-namespaced
/// iceoryx2 service names (REQ_0873); the `Kind` distinguishes the MVVM role of
/// the entry (REQ_0857).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiRouting {
    /// The logical name of the entry (ViewModel, command, …).
    pub name: String,
    /// The MVVM kind of the entry.
    pub kind: Kind,
}

impl UiRouting {
    /// Create a new routing key from a name and a [`Kind`].
    pub fn new(name: impl Into<String>, kind: Kind) -> Self {
        Self {
            name: name.into(),
            kind,
        }
    }
}

impl Routing for UiRouting {}

#[cfg(test)]
mod tests {
    use super::*;

    /// `UiRouting` must satisfy the `Routing` marker bounds so it can be used
    /// as a connector's `type Routing` (REQ_0857).
    fn assert_routing<R: Routing>() {}

    #[test]
    fn ui_routing_is_a_routing_and_carries_name_and_kind() {
        assert_routing::<UiRouting>();
        let r = UiRouting::new("Stepper", Kind::Property);
        assert_eq!(r.name, "Stepper");
        assert_eq!(r.kind, Kind::Property);
        // Routing requires Clone + Debug.
        let cloned = r.clone();
        assert_eq!(cloned, r);
        assert!(format!("{r:?}").contains("Stepper"));
    }
}
