//! The [`Client`]: binds one connector instance and drives the property and
//! command planes over iceoryx2.
//!
//! A [`Client`] is constructed with [`Client::connect`] from an instance name
//! and the contract hash the UI was built against. It reads the live manifest
//! (REQ_0872), validates the hash to pick a [`BindMode`] (REQ_0876), and then
//! exposes:
//!
//! * [`subscribe`](Client::subscribe) / [`poll_view_model`](Client::poll_view_model)
//!   — per-field [`PropertyChange`]s with staleness (REQ_0864, REQ_0880);
//! * [`invoke`](Client::invoke) — acceptance-acked commands with the epoch-aware
//!   retry policy (REQ_0865, REQ_0868, REQ_0882), disabled in read-only mode;
//! * [`subscribe_can_execute`](Client::subscribe_can_execute) /
//!   [`poll_can_execute`](Client::poll_can_execute) — the per-command gate bool.
//!
//! Everything is recovered from history-depth-1 redelivery, so a fresh `Client`
//! needs no handshake with the server (REQ_0881).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use iceoryx2::node::Node;
use iceoryx2::prelude::ipc;
use serde_json::{Map, Value};
use taktora_connector_transport_iox::envelope::CorrelationId;
use taktora_connector_transport_iox::{RawChannelReader, RawChannelWriter, ServiceFactory};
use taktora_connector_ui_contract::{Ack, CommandSchema, Manifest, RejectedCode};

use crate::ENVELOPE_CAPACITY;
use crate::binding::{BindMode, bind_mode_for};
use crate::command::{RetryDecision, mint_correlation_id, retry_decision};
use crate::discovery::{
    DEFAULT_MANIFEST_TIMEOUT, create_node, drain_latest, manifest_service_name,
    read_manifest_blocking,
};
use crate::error::ClientError;
use crate::property::{PropertyChange, Staleness, ViewModelState};

/// The interval the client busy-waits between receive attempts while blocking on
/// a reply or a first sample.
const POLL_INTERVAL: Duration = Duration::from_millis(1);

/// The command retry budget (REQ_0868).
#[derive(Clone, Copy, Debug)]
pub struct RetryPolicy {
    /// How long to wait for an ack before treating an attempt as timed out.
    pub timeout: Duration,
    /// The maximum number of send attempts (clamped to ≥ 1).
    pub max_attempts: u32,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(1),
            max_attempts: 3,
        }
    }
}

/// Client tuning knobs.
#[derive(Clone, Copy, Debug)]
pub struct ClientConfig {
    /// How long [`Client::connect`] waits for the first manifest sample.
    pub manifest_read_timeout: Duration,
    /// The command retry policy.
    pub command: RetryPolicy,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            manifest_read_timeout: DEFAULT_MANIFEST_TIMEOUT,
            command: RetryPolicy::default(),
        }
    }
}

/// The outcome of a command [`invoke`](Client::invoke).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandOutcome {
    /// The server accepted the command (at-most-once; acceptance, not
    /// completion).
    Accepted,
    /// The server rejected the command with a closed reason code.
    Rejected {
        /// The rejection reason.
        code: RejectedCode,
        /// A human-readable diagnostic.
        message: String,
    },
    /// A non-idempotent command was in flight across an epoch change (server
    /// restart): its outcome cannot be known and needs operator resolution
    /// (REQ_0882).
    OutcomeUnknown,
}

impl From<Ack> for CommandOutcome {
    fn from(ack: Ack) -> Self {
        match ack {
            Ack::Accepted => CommandOutcome::Accepted,
            Ack::Rejected { code, message } => CommandOutcome::Rejected { code, message },
        }
    }
}

/// One subscribed ViewModel: its reader plus the held-copy/staleness state.
struct VmSub {
    reader: RawChannelReader<ENVELOPE_CAPACITY>,
    state: ViewModelState,
}

/// One subscribed CanExecute gate: its reader plus the last observed bool.
struct CanSub {
    reader: RawChannelReader<ENVELOPE_CAPACITY>,
    last: Option<bool>,
}

/// One command's request publisher + reply subscriber.
struct CmdPort {
    request: RawChannelWriter<ENVELOPE_CAPACITY>,
    reply: RawChannelReader<ENVELOPE_CAPACITY>,
}

/// A bound UI client for one connector instance.
pub struct Client {
    // The node owns the iceoryx2 participant; the ports below hold their own
    // (reference-counted) service state, so the node only needs to outlive them.
    node: Node<ipc::Service>,
    manifest: Manifest,
    expected_hash: String,
    mode: BindMode,
    config: ClientConfig,
    manifest_reader: RawChannelReader<ENVELOPE_CAPACITY>,
    vms: HashMap<String, VmSub>,
    can_exec: HashMap<String, CanSub>,
    cmd_ports: HashMap<String, CmdPort>,
}

impl Client {
    /// Connect to the connector instance named `instance`, validating the
    /// client's `expected_hash` against the live manifest (REQ_0872, REQ_0876).
    ///
    /// Uses the default [`ClientConfig`].
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ManifestUnavailable`] if no manifest is published
    /// within the timeout, or a transport / codec error.
    pub fn connect(instance: &str, expected_hash: &str) -> Result<Self, ClientError> {
        Self::connect_with(instance, expected_hash, ClientConfig::default())
    }

    /// [`connect`](Client::connect) with an explicit [`ClientConfig`].
    ///
    /// # Errors
    ///
    /// As [`connect`](Client::connect).
    pub fn connect_with(
        instance: &str,
        expected_hash: &str,
        config: ClientConfig,
    ) -> Result<Self, ClientError> {
        let node = create_node()?;
        let service = manifest_service_name(instance);
        let manifest_reader = {
            let factory = ServiceFactory::new(&node);
            factory.create_raw_reader_named::<ENVELOPE_CAPACITY>(&service)?
        };
        let manifest =
            read_manifest_blocking(&manifest_reader, &service, config.manifest_read_timeout)?;
        let mode = bind_mode_for(expected_hash, &manifest);
        Ok(Self {
            node,
            manifest,
            expected_hash: expected_hash.to_owned(),
            mode,
            config,
            manifest_reader,
            vms: HashMap::new(),
            can_exec: HashMap::new(),
            cmd_ports: HashMap::new(),
        })
    }

    /// The currently-bound manifest.
    #[must_use]
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The current binding mode (read-write vs read-only).
    #[must_use]
    pub fn mode(&self) -> BindMode {
        self.mode
    }

    /// The currently-bound connector epoch.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.manifest.epoch
    }

    /// Re-read the manifest and re-validate the contract hash, rebinding the
    /// [`BindMode`] (REQ_0882). Returns `true` if the epoch changed (a restart).
    ///
    /// Cheap to call on a UI tick: the manifest republishes every server tick.
    ///
    /// # Errors
    ///
    /// Returns a transport / codec error if the latest manifest cannot be read.
    pub fn refresh_manifest(&mut self) -> Result<bool, ClientError> {
        let previous_epoch = self.manifest.epoch;
        let mut scratch = vec![0u8; ENVELOPE_CAPACITY];
        self.refresh_manifest_into(&mut scratch)?;
        Ok(self.manifest.epoch != previous_epoch)
    }

    /// Drain the manifest reader to the newest sample; if a manifest arrived,
    /// replace the held one and recompute the bind mode. Returns the current
    /// epoch.
    fn refresh_manifest_into(&mut self, scratch: &mut [u8]) -> Result<u64, ClientError> {
        if let Some((bytes, _)) = drain_latest(&self.manifest_reader, scratch)? {
            let manifest: Manifest = serde_json::from_slice(&bytes)?;
            self.mode = bind_mode_for(&self.expected_hash, &manifest);
            self.manifest = manifest;
        }
        Ok(self.manifest.epoch)
    }

    /// Subscribe to the ViewModel named `vm_name` (service from the manifest,
    /// REQ_0873). Idempotent.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::UnknownViewModel`] if `vm_name` is not in the
    /// manifest, or a transport error opening the service.
    pub fn subscribe(&mut self, vm_name: &str) -> Result<(), ClientError> {
        if self.vms.contains_key(vm_name) {
            return Ok(());
        }
        let service = self
            .manifest
            .view_models
            .iter()
            .find(|v| v.name == vm_name)
            .ok_or_else(|| ClientError::UnknownViewModel(vm_name.to_owned()))?
            .service
            .clone();
        let reader = {
            let factory = ServiceFactory::new(&self.node);
            factory.create_raw_reader_named::<ENVELOPE_CAPACITY>(&service)?
        };
        self.vms.insert(
            vm_name.to_owned(),
            VmSub {
                reader,
                state: ViewModelState::new(),
            },
        );
        Ok(())
    }

    /// Poll a subscribed ViewModel: drain to its newest value, diff it against
    /// the held copy, and return the per-field [`PropertyChange`]s (REQ_0864).
    /// Returns an empty vec when no new value arrived.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::UnknownViewModel`] if not subscribed, or a
    /// transport / codec error.
    pub fn poll_view_model(&mut self, vm_name: &str) -> Result<Vec<PropertyChange>, ClientError> {
        let mut scratch = vec![0u8; ENVELOPE_CAPACITY];
        let sub = self
            .vms
            .get_mut(vm_name)
            .ok_or_else(|| ClientError::UnknownViewModel(vm_name.to_owned()))?;
        let Some((bytes, sample)) = drain_latest(&sub.reader, &mut scratch)? else {
            return Ok(Vec::new());
        };
        let value: Value = serde_json::from_slice(&bytes)?;
        // A ViewModel is always a JSON object; ignore any non-object payload
        // rather than erroring, so one malformed frame can't wedge the UI.
        let Value::Object(map) = value else {
            return Ok(Vec::new());
        };
        let changes = sub.state.observe(
            map,
            sample.sequence_number,
            sample.timestamp_ns,
            Instant::now(),
        );
        Ok(changes)
    }

    /// The last-held field map of a subscribed ViewModel, if any value has
    /// arrived. Lets a read-only client display fields it matches by name
    /// (REQ_0876).
    #[must_use]
    pub fn view_model_fields(&self, vm_name: &str) -> Option<&Map<String, Value>> {
        self.vms.get(vm_name).map(|s| s.state.fields())
    }

    /// The staleness of a subscribed ViewModel as of now, against `threshold`
    /// (REQ_0880). [`Staleness::NeverReceived`] if unsubscribed or no value yet.
    #[must_use]
    pub fn view_model_staleness(&self, vm_name: &str, threshold: Duration) -> Staleness {
        self.vms.get(vm_name).map_or(Staleness::NeverReceived, |s| {
            s.state.staleness(Instant::now(), threshold)
        })
    }

    /// Subscribe to a command's CanExecute gate, if it has one (REQ_0866).
    /// Idempotent; a no-op (still `Ok`) for a command without a gate.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::UnknownCommand`] if `command` is not in the
    /// manifest, or a transport error opening the service.
    pub fn subscribe_can_execute(&mut self, command: &str) -> Result<(), ClientError> {
        if self.can_exec.contains_key(command) {
            return Ok(());
        }
        let schema = self
            .manifest
            .commands
            .iter()
            .find(|c| c.name == command)
            .ok_or_else(|| ClientError::UnknownCommand(command.to_owned()))?;
        let Some(service) = schema.can_execute_service.clone() else {
            return Ok(());
        };
        let reader = {
            let factory = ServiceFactory::new(&self.node);
            factory.create_raw_reader_named::<ENVELOPE_CAPACITY>(&service)?
        };
        self.can_exec
            .insert(command.to_owned(), CanSub { reader, last: None });
        Ok(())
    }

    /// Poll a subscribed CanExecute gate, returning the latest bool (or the last
    /// known value if nothing new arrived; `None` if unsubscribed / no value).
    ///
    /// # Errors
    ///
    /// Returns a transport / codec error if a sample cannot be read / parsed.
    pub fn poll_can_execute(&mut self, command: &str) -> Result<Option<bool>, ClientError> {
        let mut scratch = vec![0u8; ENVELOPE_CAPACITY];
        let Some(sub) = self.can_exec.get_mut(command) else {
            return Ok(None);
        };
        if let Some((bytes, _)) = drain_latest(&sub.reader, &mut scratch)? {
            let value: bool = serde_json::from_slice(&bytes)?;
            sub.last = Some(value);
        }
        Ok(sub.last)
    }

    /// The last-known CanExecute value for a command (no I/O).
    #[must_use]
    pub fn can_execute(&self, command: &str) -> Option<bool> {
        self.can_exec.get(command).and_then(|s| s.last)
    }

    /// Invoke `command` with JSON `params`, awaiting the acceptance ack and
    /// applying the epoch-aware retry policy (REQ_0865, REQ_0867, REQ_0868,
    /// REQ_0882).
    ///
    /// Disabled in read-only mode (REQ_0876): returns [`ClientError::ReadOnly`].
    /// A timed-out idempotent command is retried under the **same** correlation
    /// id (including across a restart); a non-idempotent command in flight across
    /// an epoch change returns [`CommandOutcome::OutcomeUnknown`].
    ///
    /// # Errors
    ///
    /// [`ClientError::ReadOnly`], [`ClientError::UnknownCommand`],
    /// [`ClientError::CommandTimeout`], or a transport / codec error.
    pub fn invoke(&mut self, command: &str, params: &Value) -> Result<CommandOutcome, ClientError> {
        if !self.mode.commands_enabled() {
            return Err(ClientError::ReadOnly);
        }
        let schema = self
            .manifest
            .commands
            .iter()
            .find(|c| c.name == command)
            .ok_or_else(|| ClientError::UnknownCommand(command.to_owned()))?
            .clone();
        self.ensure_command_port(&schema)?;

        let params_bytes = serde_json::to_vec(params)?;
        let id = mint_correlation_id();
        let start_epoch = self.manifest.epoch;
        let mut scratch = vec![0u8; ENVELOPE_CAPACITY];
        let mut attempts: u32 = 0;

        loop {
            // Resend under the same correlation id (server dedupes; REQ_0867).
            self.cmd_ports
                .get(command)
                .expect("port ensured above")
                .request
                .send_raw_bytes(&params_bytes, id)?;
            attempts += 1;

            if let Some(ack) =
                self.await_ack(command, id, self.config.command.timeout, &mut scratch)?
            {
                return Ok(ack.into());
            }

            // Timed out: re-read the manifest, re-validate the hash, rebind
            // (REQ_0882), then decide whether to retry.
            let current_epoch = self.refresh_manifest_into(&mut scratch)?;
            if !self.mode.commands_enabled() {
                // A restart changed the contract incompatibly -> read-only.
                return Err(ClientError::ReadOnly);
            }
            let epoch_changed = current_epoch != start_epoch;
            match retry_decision(
                schema.idempotent,
                epoch_changed,
                attempts,
                self.config.command.max_attempts,
            ) {
                RetryDecision::Retry => continue,
                RetryDecision::GiveUp => {
                    return Err(ClientError::CommandTimeout {
                        command: command.to_owned(),
                        attempts,
                    });
                }
                RetryDecision::OutcomeUnknown => return Ok(CommandOutcome::OutcomeUnknown),
            }
        }
    }

    /// Open a command's request writer + reply reader if not already open.
    fn ensure_command_port(&mut self, schema: &CommandSchema) -> Result<(), ClientError> {
        if self.cmd_ports.contains_key(&schema.name) {
            return Ok(());
        }
        let (request, reply) = {
            let factory = ServiceFactory::new(&self.node);
            let request =
                factory.create_raw_writer_named::<ENVELOPE_CAPACITY>(&schema.request_service)?;
            let reply =
                factory.create_raw_reader_named::<ENVELOPE_CAPACITY>(&schema.reply_service)?;
            (request, reply)
        };
        self.cmd_ports
            .insert(schema.name.clone(), CmdPort { request, reply });
        Ok(())
    }

    /// Await the ack keyed by `id` on a command's reply reader, up to `timeout`.
    /// Replies for other correlation ids (stale invocations) are drained and
    /// ignored. Returns `None` on timeout.
    fn await_ack(
        &self,
        command: &str,
        id: CorrelationId,
        timeout: Duration,
        scratch: &mut [u8],
    ) -> Result<Option<Ack>, ClientError> {
        let port = self.cmd_ports.get(command).expect("port ensured by caller");
        let deadline = Instant::now() + timeout;
        loop {
            while let Some(sample) = port.reply.try_recv_into(scratch)? {
                if sample.correlation_id == id {
                    let ack: Ack = serde_json::from_slice(&scratch[..sample.payload_len])?;
                    return Ok(Some(ack));
                }
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ack_maps_to_command_outcome() {
        assert_eq!(
            CommandOutcome::from(Ack::Accepted),
            CommandOutcome::Accepted
        );
        let rejected = Ack::Rejected {
            code: RejectedCode::CanExecuteFalse,
            message: "nope".into(),
        };
        assert_eq!(
            CommandOutcome::from(rejected),
            CommandOutcome::Rejected {
                code: RejectedCode::CanExecuteFalse,
                message: "nope".into(),
            }
        );
    }

    #[test]
    fn default_config_has_sane_bounds() {
        let cfg = ClientConfig::default();
        assert!(cfg.command.max_attempts >= 1);
        assert!(cfg.command.timeout > Duration::ZERO);
    }
}
