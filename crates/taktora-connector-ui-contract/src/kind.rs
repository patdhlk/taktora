//! The [`Kind`] discriminant tags every manifest entry as a Property, a
//! Command, a CanExecute gate, or a (reserved) Event (REQ_0857, REQ_0875).

use serde::{Deserialize, Serialize};

/// The kind of a manifest entry.
///
/// Serializes to a stable `snake_case` string tag — this tag is part of the
/// cross-language wire contract (REQ_0875) and must not change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// An observable value published latest-value over its own service.
    Property,
    /// An invocable, acceptance-acked request/response action.
    Command,
    /// A published boolean gate controlling a command's availability.
    CanExecute,
    /// Reserved for the deferred lossless event stream (not yet emitted).
    Event,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_serializes_to_stable_lowercase_tags_and_reserves_event() {
        use Kind::*;
        assert_eq!(serde_json::to_string(&Property).unwrap(), "\"property\"");
        assert_eq!(serde_json::to_string(&Command).unwrap(), "\"command\"");
        assert_eq!(
            serde_json::to_string(&CanExecute).unwrap(),
            "\"can_execute\""
        );
        assert_eq!(serde_json::to_string(&Event).unwrap(), "\"event\"");
        assert_eq!(
            serde_json::from_str::<Kind>("\"property\"").unwrap(),
            Property
        );
    }
}
