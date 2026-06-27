//! The command plane: the [`CommandParams`] authoring trait plus the off-RT
//! [`CommandHandler`] that accepts invocations, dedupes retries, gates on
//! [`CanExecute`], and enqueues effects to the application
//! (`REQ_0865`–`REQ_0871`, `REQ_0873`).
//!
//! # Authoring contract
//!
//! A command's parameter struct describes the JSON request payload a UI sends
//! to invoke the command, and whether the command is safe to auto-retry under
//! the same correlation id (`#[command(idempotent)]`). This is usually derived
//! with `#[derive(CommandParams)]`.
//!
//! # The acceptance-ack handler (`REQ_0865`, `REQ_0870`)
//!
//! [`CommandHandler`] runs on its **own** OS thread (never the executor's
//! WaitSet/RT thread, mirroring the pump). Each invocation carries a
//! [`CorrelationId`] and raw JSON params. The handler validates and replies
//! [`Ack::Accepted`] or [`Ack::Rejected`] — the reply conveys **acceptance, not
//! completion**: no per-command state is held open awaiting the effect to
//! finish. An accepted effect is only ever **enqueued** through a bounded
//! [`crossbeam_channel`] sender that the application drains on its own executor
//! task; the effect is never executed inline on the receiving thread
//! (`REQ_0870`).
//!
//! # The transport seam
//!
//! Like the pump's `VmPublisher`, the handler talks only to a
//! [`CommandTransport`] — `try_recv` for incoming invocations, `reply` for the
//! ack. Production wires [`IoxCommandTransport`] (per-command raw iceoryx2
//! request/reply services, modelled on `ZenohQueryable`); unit tests wire
//! [`MockCommandTransport`] so dedupe, back-pressure, the unknown-command path,
//! and `CanExecute` gating are all covered deterministically with no shared
//! memory and no sleeps.
//!
//! # Dedupe, back-pressure, gating
//!
//! * **Dedupe (`REQ_0867`):** a bounded LRU `correlation_id -> Ack`. Only
//!   `Accepted` acks are cached; rejections are re-evaluated on retry (a
//!   transient `BackPressure`/`CanExecuteFalse` must be retryable under the same
//!   id). A retry with a seen (accepted) id replays the cached ack **without**
//!   re-enqueuing the effect (at-most-once delivery).
//! * **Back-pressure (`REQ_0871`):** the effect channel is bounded; when full,
//!   the handler replies [`RejectedCode::BackPressure`] rather than block or
//!   drop.
//! * **Unknown command (`REQ_0869`):** an invocation naming an unregistered
//!   command replies [`RejectedCode::UnknownCommand`].
//! * **`CanExecute` (`REQ_0866`):** each command carries an [`AtomicBool`]
//!   gate, exposed to the UI as a published bool property (see
//!   [`can_execute_entry`]). An invocation while the gate is `false` replies
//!   [`RejectedCode::CanExecuteFalse`] and the effect is **not** enqueued.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, TrySendError};
use serde::de::DeserializeOwned;
use taktora_connector_core::ConnectorError;
use taktora_connector_transport_iox::{RawChannelReader, RawChannelWriter};
use taktora_connector_ui_contract::{Ack, CommandSchema, FieldSchema, Kind, RejectedCode};

use crate::pump::{EncodeFn, PumpEntry, VmPublisher};

/// The 32-byte correlation id that rides a command envelope, reused verbatim as
/// the dedupe key (`REQ_0867`). Re-exported from the iox transport so callers
/// name one type.
pub use taktora_connector_transport_iox::envelope::CorrelationId;

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

/// One command invocation delivered by a [`CommandTransport`].
///
/// Carries the command `name` to dispatch by and the raw JSON `params_json`
/// bytes; the handler validates/parses the params per registered command, so the
/// transport stays codec-agnostic and never decodes the payload itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandInvocation {
    /// The registered command name to dispatch to.
    pub name: String,
    /// The raw JSON parameter bytes (parsed per command on dispatch).
    pub params_json: Vec<u8>,
}

impl CommandInvocation {
    /// Construct an invocation from a name and raw JSON param bytes.
    #[must_use]
    pub fn new(name: impl Into<String>, params_json: impl Into<Vec<u8>>) -> Self {
        Self {
            name: name.into(),
            params_json: params_json.into(),
        }
    }
}

/// The request/reply transport seam the [`CommandHandler`] drives.
///
/// This is the command-plane analogue of the pump's
/// [`VmPublisher`]: the handler depends only on this trait,
/// so it never hard-depends on iceoryx2. Production wires
/// [`IoxCommandTransport`]; unit tests wire [`MockCommandTransport`].
pub trait CommandTransport {
    /// Try to receive one pending invocation. `None` if the inbound queue is
    /// empty. The [`CorrelationId`] is carried verbatim from the envelope and
    /// is the dedupe / reply-routing key.
    fn try_recv(&mut self) -> Option<(CorrelationId, CommandInvocation)>;

    /// Reply with the acceptance `ack` for the invocation correlated by `id`.
    ///
    /// # Errors
    ///
    /// Returns a [`ConnectorError`] if the reply could not be sent (e.g. the
    /// reply service is saturated or the transport is down).
    fn reply(&mut self, id: CorrelationId, ack: &Ack) -> Result<(), ConnectorError>;
}

/// The outcome of attempting to enqueue a command's effect.
///
/// Returned by a [`RegisteredCommand`]'s enqueue closure; the handler maps each
/// variant to an [`Ack`].
enum EnqueueOutcome {
    /// The params parsed and the effect was enqueued (`Ack::Accepted`).
    Enqueued,
    /// The params were malformed (`RejectedCode::InvalidArgs`).
    InvalidArgs(String),
    /// The effect channel was full — a transient condition the UI may retry
    /// (`RejectedCode::BackPressure`).
    BackPressure,
    /// The application dropped its receiver — a non-transient fault, not
    /// back-pressure (`RejectedCode::Faulted`).
    Faulted,
}

/// A type-erased "parse JSON params and enqueue the effect" step for one
/// command. Clears nothing; reads the raw param bytes and reports the outcome.
type EnqueueFn = Box<dyn FnMut(&[u8]) -> EnqueueOutcome + Send>;

/// A command registered with the [`CommandHandler`].
///
/// Holds the [`CanExecute`] gate (shared with the application, which flips it)
/// and a type-erased enqueue closure that parses the JSON params into the
/// command's typed parameter struct and `try_send`s it on the bounded effect
/// channel.
pub struct RegisteredCommand {
    can_execute: Arc<AtomicBool>,
    enqueue: EnqueueFn,
}

impl RegisteredCommand {
    /// Build a command that parses params into `P` and enqueues them on `sender`.
    ///
    /// Parsing failure yields [`RejectedCode::InvalidArgs`]; a full channel
    /// yields [`RejectedCode::BackPressure`] (transient, retryable) and a
    /// disconnected channel (the application dropped its receiver) yields
    /// [`RejectedCode::Faulted`] — never a block or a silent drop (`REQ_0871`).
    /// The effect is only enqueued, never run here (`REQ_0870`).
    #[must_use]
    pub fn new<P>(can_execute: &CanExecute, sender: Sender<P>) -> Self
    where
        P: DeserializeOwned + Send + 'static,
    {
        let enqueue = Box::new(
            move |bytes: &[u8]| match serde_json::from_slice::<P>(bytes) {
                Ok(parsed) => match sender.try_send(parsed) {
                    Ok(()) => EnqueueOutcome::Enqueued,
                    Err(TrySendError::Full(_)) => EnqueueOutcome::BackPressure,
                    Err(TrySendError::Disconnected(_)) => EnqueueOutcome::Faulted,
                },
                Err(e) => EnqueueOutcome::InvalidArgs(e.to_string()),
            },
        );
        Self {
            can_execute: can_execute.shared(),
            enqueue,
        }
    }
}

/// Build a bounded command channel and its [`RegisteredCommand`] in one step.
///
/// Returns the command to hand the [`CommandHandler`] and the [`Receiver<P>`] the
/// application drains on its executor task. `capacity` bounds the effect channel
/// (`REQ_0871`). This is the effect-enqueue boundary the connector's
/// `add_command` (Task 3.10) wires: register the command with the handler,
/// publish the [`CanExecute`] via [`can_execute_entry`], and hand the receiver +
/// [`CanExecute`] back to the application.
#[must_use]
pub fn command_channel<P>(
    can_execute: &CanExecute,
    capacity: usize,
) -> (RegisteredCommand, Receiver<P>)
where
    P: DeserializeOwned + Send + 'static,
{
    let (tx, rx) = crossbeam_channel::bounded(capacity);
    (RegisteredCommand::new(can_execute, tx), rx)
}

/// A command's `CanExecute` gate (`REQ_0866`).
///
/// A clone-able [`AtomicBool`] handle: the application flips it with
/// [`set`](Self::set); the [`CommandHandler`] reads it to gate acceptance; and
/// [`can_execute_entry`] publishes it as a bool property so the UI can enable /
/// disable its control. Flipping it republishes on the next pump tick (that
/// republish *is* `CanExecuteChanged`).
#[derive(Clone)]
pub struct CanExecute {
    flag: Arc<AtomicBool>,
}

impl CanExecute {
    /// A gate initialised to `initial`.
    #[must_use]
    pub fn new(initial: bool) -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(initial)),
        }
    }

    /// Set the gate. The next pump tick republishes the bool if it changed.
    pub fn set(&self, value: bool) {
        self.flag.store(value, Ordering::Release);
    }

    /// The current gate value.
    #[must_use]
    pub fn get(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }

    /// The shared [`AtomicBool`] the handler gates on and the pump publishes.
    #[must_use]
    pub fn shared(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.flag)
    }
}

impl Default for CanExecute {
    /// A gate that starts **enabled** (`true`).
    fn default() -> Self {
        Self::new(true)
    }
}

/// Build a (non-exempt) pump entry that publishes a command's [`CanExecute`]
/// state as a bare JSON bool (`REQ_0866`).
///
/// The pump coalesces, so the bool is republished only when it actually flips —
/// which is exactly the `CanExecuteChanged` notification. Like a property entry
/// it is skipped while it has zero subscribers and force-republished when a
/// subscriber (re)attaches.
#[must_use]
pub fn can_execute_entry<P>(
    name: impl Into<String>,
    can_execute: &CanExecute,
    publisher: P,
) -> PumpEntry
where
    P: VmPublisher + 'static,
{
    let flag = can_execute.shared();
    let mut last: Option<bool> = None;
    let encode: EncodeFn = Box::new(move |out: &mut Vec<u8>| {
        let value = flag.load(Ordering::Acquire);
        out.clear();
        // Serializing a bool is infallible; on the off chance it errs, report
        // "no value this tick" rather than panic on the pump thread.
        serde_json::to_writer(&mut *out, &value).ok()?;
        let changed = last != Some(value);
        last = Some(value);
        Some(changed)
    });
    PumpEntry::new(name, false, encode, Box::new(publisher))
}

/// A bounded LRU cache of `correlation_id -> Ack` for retry dedupe (`REQ_0867`).
///
/// A `get` refreshes recency; an `insert` past capacity evicts the
/// least-recently-used entry. Only `Accepted` acks are cached (the caller
/// inserts nothing else); rejections are re-evaluated on retry — a transient
/// `BackPressure`/`CanExecuteFalse` must be retryable under the same id.
/// Caching only effect-bearing acks guarantees at-most-once effect delivery: a
/// retried accepted id never re-enters dispatch, so its effect is never
/// enqueued a second time, while a rejected id remains free to succeed on retry.
struct DedupeCache {
    cap: usize,
    entries: HashMap<CorrelationId, Ack>,
    order: VecDeque<CorrelationId>,
}

impl DedupeCache {
    /// A cache holding at most `cap` entries (clamped to at least 1).
    fn new(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    /// Look up a cached ack, refreshing its recency on a hit.
    fn get(&mut self, id: &CorrelationId) -> Option<Ack> {
        let ack = self.entries.get(id).cloned()?;
        self.touch(id);
        Some(ack)
    }

    /// Insert (or update) an ack, evicting the LRU entry if over capacity.
    fn insert(&mut self, id: CorrelationId, ack: Ack) {
        // `insert` returns the prior value: `Some` means this id was already
        // cached (an update -> just refresh recency), `None` means it is new.
        if self.entries.insert(id, ack).is_some() {
            self.touch(&id);
            return;
        }
        if self.entries.len() > self.cap {
            if let Some(evicted) = self.order.pop_front() {
                self.entries.remove(&evicted);
            }
        }
        self.order.push_back(id);
    }

    /// Move `id` to the most-recently-used position.
    fn touch(&mut self, id: &CorrelationId) {
        if let Some(pos) = self.order.iter().position(|x| x == id) {
            self.order.remove(pos);
            self.order.push_back(*id);
        }
    }
}

/// The off-RT command handler (`REQ_0865`, `REQ_0867`, `REQ_0869`–`REQ_0871`).
///
/// Owns its [`CommandTransport`], the registered command set, and the dedupe
/// LRU. Drive it synchronously with [`poll`](Self::poll) (tests) or hand it to
/// its own OS thread with [`spawn`](Self::spawn) (production). It never touches
/// the executor's RT/WaitSet thread.
///
/// # Correlation-id uniqueness invariant
///
/// The single `DedupeCache` is keyed by the bare [`CorrelationId`] across
/// **all** commands. Clients MUST mint globally-unique correlation ids across
/// commands (the client mints them, `REQ_0867`); two distinct invocations
/// sharing an id — even for different commands — is a client contract
/// violation, and the second would be answered with the first's cached ack
/// rather than re-dispatched.
pub struct CommandHandler<T: CommandTransport> {
    transport: T,
    commands: HashMap<String, RegisteredCommand>,
    dedupe: DedupeCache,
}

impl<T: CommandTransport> CommandHandler<T> {
    /// A handler over `transport` with a dedupe LRU of `dedupe_capacity`
    /// (clamped to at least 1).
    #[must_use]
    pub fn new(transport: T, dedupe_capacity: usize) -> Self {
        Self {
            transport,
            commands: HashMap::new(),
            dedupe: DedupeCache::new(dedupe_capacity),
        }
    }

    /// Register `command` under `name`. Returns `&mut self` for chaining.
    ///
    /// # Panics
    ///
    /// Panics if `name` is already registered. A duplicate command name is a
    /// build-time configuration bug — the connector wires each command exactly
    /// once at setup, off the UI-input path — so it fails loudly here rather
    /// than silently overwriting the prior registration.
    pub fn register(&mut self, name: impl Into<String>, command: RegisteredCommand) -> &mut Self {
        let name = name.into();
        assert!(
            !self.commands.contains_key(&name),
            "duplicate command registration: '{name}' is already registered",
        );
        self.commands.insert(name, command);
        self
    }

    /// Borrow the transport (tests inspect the mock through this).
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Drain and handle every pending invocation, replying to each. Returns the
    /// number handled. Deterministic and synchronous — the unit tests call this
    /// directly so they need no timers.
    pub fn poll(&mut self) -> usize {
        let mut handled = 0;
        while let Some((id, invocation)) = self.transport.try_recv() {
            let ack = self.resolve(id, &invocation);
            if let Err(err) = self.transport.reply(id, &ack) {
                tracing::warn!(command = %invocation.name, error = %err, "ui command reply failed");
            }
            handled += 1;
        }
        handled
    }

    /// Resolve one invocation to an ack: replay a cached ack on a dedupe hit
    /// (no re-enqueue), otherwise dispatch and cache **only** effect-bearing
    /// (`Accepted`) acks. Rejections are deliberately *not* cached so a
    /// transient `BackPressure`/`CanExecuteFalse` is re-evaluated on retry under
    /// the same id (`REQ_0867`, `REQ_0868`).
    fn resolve(&mut self, id: CorrelationId, invocation: &CommandInvocation) -> Ack {
        if let Some(cached) = self.dedupe.get(&id) {
            return cached;
        }
        let ack = self.dispatch(invocation);
        if matches!(ack, Ack::Accepted) {
            self.dedupe.insert(id, ack.clone());
        }
        ack
    }

    /// Dispatch a not-yet-seen invocation: unknown command, then the
    /// `CanExecute` gate, then parse + enqueue.
    fn dispatch(&mut self, invocation: &CommandInvocation) -> Ack {
        let Some(command) = self.commands.get_mut(&invocation.name) else {
            return rejected(
                RejectedCode::UnknownCommand,
                format!("no command named '{}'", invocation.name),
            );
        };
        if !command.can_execute.load(Ordering::Acquire) {
            return rejected(
                RejectedCode::CanExecuteFalse,
                format!("command '{}' is not currently executable", invocation.name),
            );
        }
        match (command.enqueue)(&invocation.params_json) {
            EnqueueOutcome::Enqueued => Ack::Accepted,
            EnqueueOutcome::InvalidArgs(message) => rejected(RejectedCode::InvalidArgs, message),
            EnqueueOutcome::BackPressure => rejected(
                RejectedCode::BackPressure,
                format!("command '{}' effect channel is full", invocation.name),
            ),
            EnqueueOutcome::Faulted => rejected(
                RejectedCode::Faulted,
                format!(
                    "command '{}' effect channel is disconnected (application receiver dropped)",
                    invocation.name
                ),
            ),
        }
    }
}

impl<T: CommandTransport + Send + 'static> CommandHandler<T> {
    /// Spawn the handler on its own OS thread, polling every `poll_interval`.
    ///
    /// Runs until [`CommandHandlerHandle::stop`] is called, then performs one
    /// final drain so a just-arrived invocation is still answered before exit
    /// (mirroring the pump's final-tick drain).
    pub fn spawn(mut self, poll_interval: Duration) -> CommandHandlerHandle {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let handle = thread::spawn(move || -> u64 {
            let mut total: u64 = 0;
            loop {
                let stopping = stop_thread.load(Ordering::Acquire);
                total += self.poll() as u64;
                if stopping {
                    break;
                }
                thread::sleep(poll_interval);
            }
            total
        });
        CommandHandlerHandle { stop, handle }
    }
}

/// Handle to a running [`CommandHandler`] thread. Call [`stop`](Self::stop) to
/// drain once more and join.
#[must_use = "call stop() to drain and join the handler thread; dropping it leaks the thread"]
pub struct CommandHandlerHandle {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<u64>,
}

impl CommandHandlerHandle {
    /// Signal the handler to perform one final drain, then join it. Returns the
    /// total number of invocations handled over the thread's lifetime, or `0` if
    /// the handler thread had panicked.
    ///
    /// A join error is logged and swallowed rather than propagated as a panic:
    /// this runs on [`UiConnector`](crate::UiConnector)'s `Drop` path, where a
    /// panic would be a panic-in-`Drop` (process abort). The happy path is
    /// unchanged.
    pub fn stop(self) -> u64 {
        self.stop.store(true, Ordering::Release);
        match self.handle.join() {
            Ok(total) => total,
            Err(_) => {
                tracing::error!(
                    "ui command handler thread panicked; ignoring join error during shutdown"
                );
                0
            }
        }
    }
}

/// The maximum byte length of a [`Ack::Rejected`] `message`.
///
/// Rejection messages echo caller-controlled input (a command name, a serde
/// parse error) and can be arbitrarily long. The encoded ack must fit the
/// command envelope's payload capacity `N`, or [`reply`](CommandTransport::reply)
/// returns [`ConnectorError::PayloadOverflow`] — which `poll` only logs, leaving
/// the UI with no ack at all (a timeout). Truncating every message to this fixed
/// cap keeps the encoded ack within `N` so the UI always gets an answer.
const ACK_MESSAGE_CAP: usize = 200;

/// Helper: build a rejection ack, truncating an oversized `message` to
/// [`ACK_MESSAGE_CAP`] bytes on a UTF-8 char boundary so the encoded ack always
/// fits the envelope.
fn rejected(code: RejectedCode, mut message: String) -> Ack {
    if message.len() > ACK_MESSAGE_CAP {
        let mut end = ACK_MESSAGE_CAP;
        while end > 0 && !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
    }
    Ack::Rejected { code, message }
}

/// One command's iceoryx2 request/reply port pair.
struct IoxCommandPort<const N: usize> {
    name: String,
    reader: RawChannelReader<N>,
    writer: RawChannelWriter<N>,
}

/// The production [`CommandTransport`], modelled on `ZenohQueryable`.
///
/// Each registered command owns a raw iceoryx2 **request** subscriber (the UI's
/// invocations arrive here) and a raw **reply** publisher (the ack goes back),
/// matching the per-command `request_service` / `reply_service` names in the
/// manifest (`REQ_0873`). The [`CorrelationId`] rides the envelope verbatim, so
/// the handler's dedupe key and the reply routing key are the same 32 bytes.
///
/// `try_recv` round-robins the request readers for fairness and records which
/// port each correlation id arrived on; `reply` looks that up to publish the ack
/// on the matching reply service. `N` is the command envelope payload capacity
/// (large enough for both the JSON params and the JSON [`Ack`]).
///
/// This is the minimal wire impl — the deterministic behaviour (dedupe,
/// back-pressure, gating, unknown command) is covered by unit tests over
/// [`MockCommandTransport`]; the heavy shared-memory round-trip lives in
/// `taktora-connector-ui-tests`.
///
/// # Correlation-id uniqueness invariant
///
/// `pending` is keyed by the bare [`CorrelationId`] across **all** command
/// ports, so clients MUST mint globally-unique correlation ids across commands
/// (the client mints them, `REQ_0867`). Two in-flight invocations sharing an id
/// — even on different command ports — is a client contract violation: the
/// second overwrites the first's pending reply-routing entry.
pub struct IoxCommandTransport<const N: usize> {
    commands: Vec<IoxCommandPort<N>>,
    pending: HashMap<CorrelationId, usize>,
    cursor: usize,
    recv_scratch: Vec<u8>,
}

impl<const N: usize> Default for IoxCommandTransport<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> IoxCommandTransport<N> {
    /// An empty transport with no command ports.
    #[must_use]
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            pending: HashMap::new(),
            cursor: 0,
            recv_scratch: vec![0u8; N],
        }
    }

    /// Register a command's request reader + reply writer under `name`.
    ///
    /// The connector (Task 3.10) opens these raw handles against the
    /// instance-namespaced `request_service` / `reply_service` and hands them
    /// here. Returns `&mut self` for chaining.
    ///
    /// # Panics
    ///
    /// Panics if `name` is already registered. A duplicate command name is a
    /// build-time configuration bug (commands are wired once at setup), so it
    /// fails loudly here rather than adding a shadow port that could never be
    /// reached by the round-robin receive.
    pub fn add_command(
        &mut self,
        name: impl Into<String>,
        reader: RawChannelReader<N>,
        writer: RawChannelWriter<N>,
    ) -> &mut Self {
        let name = name.into();
        assert!(
            !self.commands.iter().any(|c| c.name == name),
            "duplicate command registration: '{name}' is already registered",
        );
        self.commands.push(IoxCommandPort {
            name,
            reader,
            writer,
        });
        self
    }
}

impl<const N: usize> CommandTransport for IoxCommandTransport<N> {
    fn try_recv(&mut self) -> Option<(CorrelationId, CommandInvocation)> {
        let count = self.commands.len();
        for offset in 0..count {
            let idx = (self.cursor + offset) % count;
            match self.commands[idx]
                .reader
                .try_recv_into(&mut self.recv_scratch)
            {
                Ok(Some(sample)) => {
                    self.cursor = (idx + 1) % count;
                    let invocation = CommandInvocation::new(
                        self.commands[idx].name.clone(),
                        self.recv_scratch[..sample.payload_len].to_vec(),
                    );
                    self.pending.insert(sample.correlation_id, idx);
                    return Some((sample.correlation_id, invocation));
                }
                Ok(None) => {}
                Err(err) => {
                    tracing::warn!(
                        command = %self.commands[idx].name,
                        error = %err,
                        "ui command request receive failed"
                    );
                }
            }
        }
        None
    }

    fn reply(&mut self, id: CorrelationId, ack: &Ack) -> Result<(), ConnectorError> {
        let idx = self.pending.remove(&id).ok_or_else(|| {
            ConnectorError::Configuration("no pending command for correlation id".to_owned())
        })?;
        let encoded = serde_json::to_vec(ack).map_err(|e| ConnectorError::Codec {
            format: "json",
            source: Box::new(e),
        })?;
        if encoded.len() > N {
            return Err(ConnectorError::PayloadOverflow {
                actual: encoded.len(),
                max: N,
            });
        }
        // `encoded` is already a contiguous buffer; send it directly rather than
        // copying through a scratch intermediate.
        self.commands[idx].writer.send_raw_bytes(&encoded, id)?;
        Ok(())
    }
}

/// A test [`CommandTransport`] backed by an in-memory queue.
///
/// Mirrors [`MockPublisher`](crate::MockPublisher): clone-able, every clone
/// shares one backing state, so a test keeps one handle to push invocations and
/// inspect recorded replies while a clone is moved into the [`CommandHandler`].
#[derive(Clone, Default)]
pub struct MockCommandTransport {
    inner: Arc<MockTransportState>,
}

#[derive(Default)]
struct MockTransportState {
    incoming: std::sync::Mutex<VecDeque<(CorrelationId, CommandInvocation)>>,
    replies: std::sync::Mutex<Vec<(CorrelationId, Ack)>>,
    fail_reply: AtomicBool,
}

impl MockCommandTransport {
    /// An empty transport.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue one invocation to be returned by a later
    /// [`try_recv`](CommandTransport::try_recv).
    pub fn push(&self, id: CorrelationId, invocation: CommandInvocation) {
        self.inner
            .incoming
            .lock()
            .expect("mock lock")
            .push_back((id, invocation));
    }

    /// Make subsequent [`reply`](CommandTransport::reply) calls fail (`on`) or
    /// succeed (`off`).
    pub fn set_fail_reply(&self, on: bool) {
        self.inner.fail_reply.store(on, Ordering::Relaxed);
    }

    /// Every `(id, ack)` replied so far, in order.
    #[must_use]
    pub fn replies(&self) -> Vec<(CorrelationId, Ack)> {
        self.inner.replies.lock().expect("mock lock").clone()
    }

    /// The most recently replied ack, if any.
    #[must_use]
    pub fn last_ack(&self) -> Option<Ack> {
        self.inner
            .replies
            .lock()
            .expect("mock lock")
            .last()
            .map(|(_, ack)| ack.clone())
    }
}

impl CommandTransport for MockCommandTransport {
    fn try_recv(&mut self) -> Option<(CorrelationId, CommandInvocation)> {
        self.inner.incoming.lock().expect("mock lock").pop_front()
    }

    fn reply(&mut self, id: CorrelationId, ack: &Ack) -> Result<(), ConnectorError> {
        if self.inner.fail_reply.load(Ordering::Relaxed) {
            return Err(ConnectorError::Down {
                reason: "mock reply failure".into(),
            });
        }
        self.inner
            .replies
            .lock()
            .expect("mock lock")
            .push((id, ack.clone()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    /// A simple command param type used across the tests.
    #[derive(Clone, Debug, PartialEq, Deserialize)]
    struct Jog {
        delta: f64,
    }

    fn corr(n: u8) -> CorrelationId {
        let mut id = [0u8; 32];
        id[0] = n;
        id
    }

    fn jog_invocation(delta: f64) -> CommandInvocation {
        CommandInvocation::new("jog", format!("{{\"delta\":{delta}}}").into_bytes())
    }

    /// Wire one handler with a single `jog` command. Returns the handler, the
    /// shared mock transport, the effect receiver, and the `CanExecute` gate.
    fn handler_with_jog(
        capacity: usize,
        can_initial: bool,
    ) -> (
        CommandHandler<MockCommandTransport>,
        MockCommandTransport,
        Receiver<Jog>,
        CanExecute,
    ) {
        let transport = MockCommandTransport::new();
        let can = CanExecute::new(can_initial);
        let (command, rx) = command_channel::<Jog>(&can, capacity);
        let mut handler = CommandHandler::new(transport.clone(), 16);
        handler.register("jog", command);
        (handler, transport, rx, can)
    }

    #[test]
    fn accepted_path_enqueues_effect_and_replies_accepted() {
        let (mut handler, transport, rx, _can) = handler_with_jog(4, true);
        transport.push(corr(1), jog_invocation(1.5));

        assert_eq!(handler.poll(), 1);

        assert_eq!(transport.last_ack(), Some(Ack::Accepted));
        assert_eq!(rx.try_recv().ok(), Some(Jog { delta: 1.5 }));
    }

    #[test]
    fn dedupe_replays_cached_ack_and_does_not_reenqueue() {
        let (mut handler, transport, rx, _can) = handler_with_jog(4, true);

        // First delivery: accepted + one effect enqueued.
        transport.push(corr(7), jog_invocation(2.0));
        handler.poll();
        // Retry under the SAME correlation id.
        transport.push(corr(7), jog_invocation(2.0));
        handler.poll();

        // Both replies are Accepted (the second is the replayed cache hit)...
        let replies = transport.replies();
        assert_eq!(replies.len(), 2);
        assert!(replies.iter().all(|(_, ack)| *ack == Ack::Accepted));
        // ...but the effect was enqueued exactly once (no re-enqueue).
        assert_eq!(rx.try_recv().ok(), Some(Jog { delta: 2.0 }));
        assert!(
            rx.try_recv().is_err(),
            "retry must not re-enqueue the effect"
        );
    }

    #[test]
    fn full_channel_rejects_with_backpressure() {
        // Capacity 1: the first invocation fills the channel; the second (a
        // different correlation id, never drained) gets BackPressure.
        let (mut handler, transport, _rx, _can) = handler_with_jog(1, true);
        transport.push(corr(1), jog_invocation(1.0));
        transport.push(corr(2), jog_invocation(2.0));

        handler.poll();

        let replies = transport.replies();
        assert_eq!(replies[0].1, Ack::Accepted);
        match &replies[1].1 {
            Ack::Rejected { code, .. } => assert_eq!(*code, RejectedCode::BackPressure),
            other => panic!("expected back-pressure rejection, got {other:?}"),
        }
    }

    #[test]
    fn transient_backpressure_is_retryable_under_same_id() {
        // REQ_0867/REQ_0868: a rejection must NOT poison the correlation id.
        // Capacity 1: a first effect fills the channel, so an invocation under
        // id X is rejected with BackPressure; once the application drains the
        // channel, retrying the SAME id X must succeed and enqueue the effect.
        let (mut handler, transport, rx, _can) = handler_with_jog(1, true);

        // Fill the single channel slot via a distinct id.
        transport.push(corr(1), jog_invocation(1.0));
        handler.poll();
        assert_eq!(transport.last_ack(), Some(Ack::Accepted));

        // id X (corr 2) hits the now-full channel -> BackPressure.
        transport.push(corr(2), jog_invocation(2.0));
        handler.poll();
        match transport.last_ack().unwrap() {
            Ack::Rejected { code, .. } => assert_eq!(code, RejectedCode::BackPressure),
            other => panic!("expected back-pressure rejection, got {other:?}"),
        }

        // Application catches up and drains the channel.
        assert_eq!(rx.try_recv().ok(), Some(Jog { delta: 1.0 }));

        // Retry the SAME id X: the transient rejection must not be cached, so
        // this re-dispatches, is Accepted, and the effect IS enqueued.
        transport.push(corr(2), jog_invocation(2.0));
        handler.poll();
        assert_eq!(
            transport.last_ack(),
            Some(Ack::Accepted),
            "a transient BackPressure must be retryable under the same id"
        );
        assert_eq!(
            rx.try_recv().ok(),
            Some(Jog { delta: 2.0 }),
            "the retry must enqueue the effect"
        );
    }

    #[test]
    fn unknown_command_is_rejected() {
        let (mut handler, transport, _rx, _can) = handler_with_jog(4, true);
        transport.push(
            corr(1),
            CommandInvocation::new("does_not_exist", b"{}".to_vec()),
        );

        handler.poll();

        match transport.last_ack().unwrap() {
            Ack::Rejected { code, .. } => assert_eq!(code, RejectedCode::UnknownCommand),
            other => panic!("expected rejection, got {other:?}"),
        }
    }

    #[test]
    fn malformed_params_are_rejected_invalid_args() {
        let (mut handler, transport, rx, _can) = handler_with_jog(4, true);
        // "delta" must be a number; a string is the wrong shape.
        transport.push(
            corr(1),
            CommandInvocation::new("jog", b"{\"delta\":\"oops\"}".to_vec()),
        );

        handler.poll();

        match transport.last_ack().unwrap() {
            Ack::Rejected { code, .. } => assert_eq!(code, RejectedCode::InvalidArgs),
            other => panic!("expected rejection, got {other:?}"),
        }
        assert!(rx.try_recv().is_err(), "malformed params must not enqueue");
    }

    #[test]
    fn can_execute_false_rejects_and_does_not_enqueue() {
        let (mut handler, transport, rx, can) = handler_with_jog(4, false);
        transport.push(corr(1), jog_invocation(1.0));

        handler.poll();

        match transport.last_ack().unwrap() {
            Ack::Rejected { code, .. } => assert_eq!(code, RejectedCode::CanExecuteFalse),
            other => panic!("expected rejection, got {other:?}"),
        }
        assert!(rx.try_recv().is_err(), "gated command must not enqueue");

        // Flipping the gate true lets a fresh invocation through.
        can.set(true);
        transport.push(corr(2), jog_invocation(3.0));
        handler.poll();
        assert_eq!(transport.last_ack(), Some(Ack::Accepted));
        assert_eq!(rx.try_recv().ok(), Some(Jog { delta: 3.0 }));
    }

    #[test]
    fn effect_is_only_enqueued_never_run_inline() {
        // The handler must never execute the effect on its own thread (REQ_0870):
        // after poll, the parsed params sit in the channel undrained — nothing
        // ran them.
        let (mut handler, transport, rx, _can) = handler_with_jog(4, true);
        transport.push(corr(1), jog_invocation(9.0));

        handler.poll();

        // The effect is parked in the channel (proving "enqueued, not executed").
        assert_eq!(rx.len(), 1, "effect must be enqueued exactly once");
        assert_eq!(rx.try_recv().ok(), Some(Jog { delta: 9.0 }));
    }

    #[test]
    fn distinct_correlation_ids_each_enqueue() {
        let (mut handler, transport, rx, _can) = handler_with_jog(4, true);
        transport.push(corr(1), jog_invocation(1.0));
        transport.push(corr(2), jog_invocation(2.0));

        handler.poll();

        assert_eq!(rx.try_recv().ok(), Some(Jog { delta: 1.0 }));
        assert_eq!(rx.try_recv().ok(), Some(Jog { delta: 2.0 }));
    }

    #[test]
    fn reply_failure_is_swallowed_not_panicked() {
        let (mut handler, transport, _rx, _can) = handler_with_jog(4, true);
        transport.set_fail_reply(true);
        transport.push(corr(1), jog_invocation(1.0));

        // poll must not panic even though every reply errors.
        assert_eq!(handler.poll(), 1);
        assert!(transport.replies().is_empty());
    }

    #[test]
    fn disconnected_channel_rejects_with_faulted() {
        // A dropped application receiver is a non-transient fault, not
        // back-pressure: the handler must reject with Faulted, not BackPressure.
        let (mut handler, transport, rx, _can) = handler_with_jog(4, true);
        drop(rx); // application dropped its receiver.
        transport.push(corr(1), jog_invocation(1.0));

        handler.poll();

        match transport.last_ack().unwrap() {
            Ack::Rejected { code, .. } => assert_eq!(code, RejectedCode::Faulted),
            other => panic!("expected faulted rejection, got {other:?}"),
        }
    }

    #[test]
    fn oversized_rejection_message_is_capped() {
        // An arbitrarily long (caller-controlled) rejection message must be
        // truncated so the encoded ack always fits the envelope; otherwise the
        // UI would get no ack at all (a PayloadOverflow the poll loop only logs).
        let (mut handler, transport, _rx, _can) = handler_with_jog(4, true);
        let long_name = "x".repeat(1000);
        transport.push(corr(1), CommandInvocation::new(long_name, b"{}".to_vec()));

        handler.poll();

        match transport.last_ack().unwrap() {
            Ack::Rejected { code, message } => {
                assert_eq!(code, RejectedCode::UnknownCommand);
                assert!(
                    message.len() <= ACK_MESSAGE_CAP,
                    "rejection message must be capped at {ACK_MESSAGE_CAP} bytes, got {}",
                    message.len()
                );
            }
            other => panic!("expected rejection, got {other:?}"),
        }
    }

    #[test]
    #[should_panic(expected = "duplicate command registration")]
    fn duplicate_registration_panics() {
        let transport = MockCommandTransport::new();
        let can = CanExecute::new(true);
        let (command_a, _rx_a) = command_channel::<Jog>(&can, 4);
        let (command_b, _rx_b) = command_channel::<Jog>(&can, 4);
        let mut handler = CommandHandler::new(transport, 16);
        handler.register("jog", command_a);
        handler.register("jog", command_b); // duplicate name -> panic.
    }

    #[test]
    fn dedupe_lru_evicts_oldest_beyond_capacity() {
        let transport = MockCommandTransport::new();
        let can = CanExecute::new(true);
        let (command, rx) = command_channel::<Jog>(&can, 64);
        let mut handler = CommandHandler::new(transport.clone(), 2);
        handler.register("jog", command);

        // Fill the 2-slot dedupe with ids 1 and 2, then push id 3 (evicts 1).
        transport.push(corr(1), jog_invocation(1.0));
        transport.push(corr(2), jog_invocation(2.0));
        transport.push(corr(3), jog_invocation(3.0));
        handler.poll();
        // Drain the three accepted effects.
        for _ in 0..3 {
            assert!(rx.try_recv().is_ok());
        }

        // Re-sending id 1 is now a cache MISS (it was evicted) -> re-enqueues.
        transport.push(corr(1), jog_invocation(1.0));
        handler.poll();
        assert_eq!(
            rx.try_recv().ok(),
            Some(Jog { delta: 1.0 }),
            "evicted id must re-dispatch (LRU dropped it)"
        );

        // Re-sending id 3 is still a cache HIT (most recent) -> no re-enqueue.
        transport.push(corr(3), jog_invocation(3.0));
        handler.poll();
        assert!(
            rx.try_recv().is_err(),
            "retained id must replay the cache, not re-enqueue"
        );
    }

    #[test]
    fn spawn_then_stop_drains_and_joins() {
        let (handler, transport, rx, _can) = handler_with_jog(8, true);
        transport.push(corr(1), jog_invocation(4.0));

        let handle = handler.spawn(Duration::from_millis(2));
        // Give the thread a few polls, then push a late invocation.
        thread::sleep(Duration::from_millis(20));
        transport.push(corr(2), jog_invocation(5.0));
        let total = handle.stop();

        assert!(total >= 2, "handler should have handled both invocations");
        assert_eq!(rx.try_recv().ok(), Some(Jog { delta: 4.0 }));
        assert_eq!(rx.try_recv().ok(), Some(Jog { delta: 5.0 }));
    }

    // --- CanExecute as a published bool property (Task 3.8) ---

    mod can_execute_property {
        use super::*;
        use crate::pump::{MockPublisher, Pump};

        fn parse_bool(bytes: &[u8]) -> bool {
            serde_json::from_slice(bytes).unwrap()
        }

        #[test]
        fn publishes_initial_bool_then_republishes_only_on_change() {
            let can = CanExecute::new(true);
            let publisher = MockPublisher::with_subscribers(1);
            let mut pump = Pump::new();
            pump.add_entry(can_execute_entry("jog.can", &can, publisher.clone()));

            // First tick publishes the initial value.
            assert_eq!(pump.tick().published, 1);
            assert!(parse_bool(&publisher.last_published().unwrap()));

            // Unchanged -> coalesced (no republish).
            assert_eq!(pump.tick().published, 0);
            assert_eq!(publisher.publish_count(), 1);

            // Flip -> republished (this republish IS CanExecuteChanged).
            can.set(false);
            assert_eq!(pump.tick().published, 1);
            assert!(!parse_bool(&publisher.last_published().unwrap()));
            assert_eq!(publisher.publish_count(), 2);
        }

        #[test]
        fn gate_and_published_property_share_state() {
            // The same CanExecute drives both the handler gate and the published
            // bool, so a UI's view of executability matches the handler's
            // acceptance decision.
            let can = CanExecute::new(false);
            let publisher = MockPublisher::with_subscribers(1);
            let mut pump = Pump::new();
            pump.add_entry(can_execute_entry("jog.can", &can, publisher.clone()));

            pump.tick();
            assert!(!parse_bool(&publisher.last_published().unwrap()));
            assert!(!can.get());

            can.set(true);
            pump.tick();
            assert!(parse_bool(&publisher.last_published().unwrap()));
            assert!(can.get());
        }
    }
}
