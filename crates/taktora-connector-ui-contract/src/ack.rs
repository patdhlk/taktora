//! Command acceptance acks and the closed set of rejection reason codes
//! (REQ_0869, REQ_0865).

use serde::{Deserialize, Serialize};

/// The closed set of reasons a command invocation may be rejected.
///
/// Each variant serializes to a stable `snake_case` tag that is part of the
/// cross-language wire contract (REQ_0865) and must not change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectedCode {
    /// The command's CanExecute gate was `false` at acceptance time.
    CanExecuteFalse,
    /// The supplied parameters were malformed or out of range.
    InvalidArgs,
    /// The application is in a faulted state and cannot accept the command.
    Faulted,
    /// The command channel was full (backpressure).
    BackPressure,
    /// No command with the requested name is registered.
    UnknownCommand,
    /// The invocation's contract hash did not match the published manifest.
    ContractMismatch,
}

/// A command acceptance ack (REQ_0869).
///
/// Adjacently tagged on `"ack"`: `Accepted` is `{"ack":"accepted"}`, and
/// `Rejected` carries a [`RejectedCode`] and a human-readable message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "ack", rename_all = "snake_case")]
pub enum Ack {
    /// The command was accepted for execution (at-most-once).
    Accepted,
    /// The command was rejected; the effect did not run.
    Rejected {
        /// The closed reason code.
        code: RejectedCode,
        /// A human-readable diagnostic message.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_code_serializes_to_stable_snake_case_tags() {
        assert_eq!(
            serde_json::to_string(&RejectedCode::CanExecuteFalse).unwrap(),
            "\"can_execute_false\""
        );
        assert_eq!(
            serde_json::to_string(&RejectedCode::InvalidArgs).unwrap(),
            "\"invalid_args\""
        );
        assert_eq!(
            serde_json::to_string(&RejectedCode::Faulted).unwrap(),
            "\"faulted\""
        );
        assert_eq!(
            serde_json::to_string(&RejectedCode::BackPressure).unwrap(),
            "\"back_pressure\""
        );
        assert_eq!(
            serde_json::to_string(&RejectedCode::UnknownCommand).unwrap(),
            "\"unknown_command\""
        );
        assert_eq!(
            serde_json::to_string(&RejectedCode::ContractMismatch).unwrap(),
            "\"contract_mismatch\""
        );
    }

    #[test]
    fn ack_uses_adjacent_ack_tag_and_round_trips() {
        let accepted = Ack::Accepted;
        let j = serde_json::to_value(&accepted).unwrap();
        assert_eq!(j, serde_json::json!({"ack": "accepted"}));
        assert_eq!(serde_json::from_value::<Ack>(j).unwrap(), accepted);

        let rejected = Ack::Rejected {
            code: RejectedCode::BackPressure,
            message: "queue full".into(),
        };
        let j = serde_json::to_value(&rejected).unwrap();
        assert_eq!(j["ack"], "rejected");
        assert_eq!(j["code"], "back_pressure");
        assert_eq!(j["message"], "queue full");
        assert_eq!(serde_json::from_value::<Ack>(j).unwrap(), rejected);
    }
}
