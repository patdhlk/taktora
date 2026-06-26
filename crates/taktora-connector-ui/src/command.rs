//! The [`CommandParams`] authoring trait (`REQ_0868`, `REQ_0873`).
//!
//! A command's parameter struct describes the JSON request payload a UI sends
//! to invoke the command, and whether the command is safe to auto-retry under
//! the same correlation id (`#[command(idempotent)]`). This is usually derived
//! with `#[derive(CommandParams)]`.
//!
//! The command handler, dedupe LRU, and `CanExecute` gating land in a later
//! slice; this module defines only the authoring contract the derive targets.

use taktora_connector_ui_contract::{CommandSchema, FieldSchema, Kind};

/// The authoring contract for a command's parameter struct.
///
/// Implemented (usually via `#[derive(CommandParams)]`) by a command's request
/// struct. It contributes the parameter [`FieldSchema`] list to the manifest
/// and the [`IDEMPOTENT`](CommandParams::IDEMPOTENT) flag.
pub trait CommandParams: Sized {
    /// Whether the command is safe to auto-retry under the same correlation id
    /// (set by `#[command(idempotent)]`; defaults to `false`).
    const IDEMPOTENT: bool;

    /// The parameter fields, in declaration order, lowered to the closed POD
    /// [`FieldSchema`] set.
    fn params() -> Vec<FieldSchema>;

    /// Assemble the full [`CommandSchema`] contribution for the manifest.
    ///
    /// The connector supplies the instance-namespaced service names (`REQ_0873`);
    /// the kind is always [`Kind::Command`] and the idempotent flag comes from
    /// [`IDEMPOTENT`](CommandParams::IDEMPOTENT).
    #[must_use]
    fn command_schema(
        name: impl Into<String>,
        request_service: impl Into<String>,
        reply_service: impl Into<String>,
        can_execute_service: Option<String>,
    ) -> CommandSchema {
        CommandSchema {
            name: name.into(),
            request_service: request_service.into(),
            reply_service: reply_service.into(),
            params: Self::params(),
            kind: Kind::Command,
            idempotent: Self::IDEMPOTENT,
            can_execute_service,
        }
    }
}
