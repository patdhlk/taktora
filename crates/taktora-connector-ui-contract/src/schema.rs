//! The manifest schema types: [`ViewModelSchema`], [`CommandSchema`], and the
//! top-level [`Manifest`] (REQ_0872, REQ_0873).

use serde::{Deserialize, Serialize};

use crate::field::FieldSchema;
use crate::kind::Kind;

/// The schema of one ViewModel: a fixed-layout POD struct published
/// latest-value over a single service.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ViewModelSchema {
    /// The ViewModel's logical name.
    pub name: String,
    /// The fully-qualified, instance-namespaced service name it publishes on.
    pub service: String,
    /// The struct's fields, in declaration order.
    pub fields: Vec<FieldSchema>,
}

/// The schema of one command: an acceptance-acked request/response action.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommandSchema {
    /// The command's logical name.
    pub name: String,
    /// The service carrying invocation requests.
    pub request_service: String,
    /// The service carrying acceptance acks.
    pub reply_service: String,
    /// The command's parameter fields, in declaration order.
    pub params: Vec<FieldSchema>,
    /// The entry kind (always [`Kind::Command`] for a command).
    pub kind: Kind,
    /// Whether the command is safe to auto-retry under the same correlation id.
    pub idempotent: bool,
    /// The optional CanExecute gate property service, if the command has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_execute_service: Option<String>,
}

/// The self-describing manifest a UI process binds against (REQ_0872).
///
/// It is the sole source of service names (REQ_0873) and carries a structural
/// [`contract_hash`](Manifest::contract_hash) for compatibility validation
/// (REQ_0874).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Manifest {
    /// The instance namespace prefixing every service name.
    pub instance: String,
    /// A process-unique epoch identifying this connector incarnation.
    pub epoch: u64,
    /// The lowercase-hex structural contract hash (see `crate::hash`).
    pub contract_hash: String,
    /// The published ViewModels.
    pub view_models: Vec<ViewModelSchema>,
    /// The available commands.
    pub commands: Vec<CommandSchema>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::{FieldSchema, FieldType};
    use crate::kind::Kind;

    fn sample_manifest() -> Manifest {
        Manifest {
            instance: "stepper-app".into(),
            epoch: 7,
            contract_hash: "deadbeef".into(),
            view_models: vec![ViewModelSchema {
                name: "Stepper".into(),
                service: "stepper-app/vm/Stepper".into(),
                fields: vec![FieldSchema {
                    name: "position".into(),
                    ty: FieldType::F64,
                }],
            }],
            commands: vec![CommandSchema {
                name: "enable".into(),
                request_service: "stepper-app/cmd/enable/req".into(),
                reply_service: "stepper-app/cmd/enable/rep".into(),
                params: vec![FieldSchema {
                    name: "force".into(),
                    ty: FieldType::Bool,
                }],
                kind: Kind::Command,
                idempotent: true,
                can_execute_service: Some("stepper-app/cmd/enable/can".into()),
            }],
        }
    }

    #[test]
    fn manifest_carries_identity_fields_and_round_trips() {
        let m = sample_manifest();
        let j = serde_json::to_value(&m).unwrap();
        assert_eq!(j["instance"], "stepper-app");
        assert_eq!(j["epoch"], 7);
        assert_eq!(j["contract_hash"], "deadbeef");

        let cmd = &j["commands"][0];
        assert_eq!(cmd["request_service"], "stepper-app/cmd/enable/req");
        assert_eq!(cmd["reply_service"], "stepper-app/cmd/enable/rep");
        assert_eq!(cmd["kind"], "command");
        assert_eq!(cmd["idempotent"], true);
        assert_eq!(cmd["can_execute_service"], "stepper-app/cmd/enable/can");
        assert!(cmd["params"].is_array());

        let back: Manifest = serde_json::from_value(j).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn absent_can_execute_service_is_omitted_from_json() {
        let mut m = sample_manifest();
        m.commands[0].can_execute_service = None;
        let j = serde_json::to_value(&m).unwrap();
        assert!(j["commands"][0].get("can_execute_service").is_none());
        let back: Manifest = serde_json::from_value(j).unwrap();
        assert_eq!(back, m);
    }
}
