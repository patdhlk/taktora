UI connector (MVVM)
===================

Test cases verifying the UI connector requirements
(:need:`REQ_0855`–:need:`REQ_0884`). The deterministic, host-OS-agnostic
behaviour (derive layout, schema, dedupe, back-pressure, gating, health) is
covered by unit tests and ``trybuild`` compile-fail fixtures that run in
default CI on Linux, macOS, and Windows. The live publish/command/discovery
round-trips stand up real iceoryx2 shared-memory services, so they live in the
``publish = false`` ``taktora-connector-ui-tests`` crate (run under the
workspace's single-test-thread discipline, :need:`CON_0004`). The
language-neutral wire contract is pinned by a checked-in golden manifest and a
reproducibility check that recomputes the contract hash from the JSON alone.

.. test:: ViewModel derive: layout, schema and envelope sizing
   :id: TEST_0873
   :status: implemented
   :verifies: REQ_0859, REQ_0878

   ``#[derive(ViewModel)]`` / ``#[derive(ImageEnum)]`` over a struct of
   ``bool`` / ``f64`` / fixed array / ``BoundedString`` / C-like enum
   fields, asserting the derived compile-time envelope size ``N`` matches
   the declared layout (:need:`REQ_0859`) and that the emitted
   ``ViewModelSchema`` / ``FieldType`` manifest contribution names every
   field with the correct descriptor (:need:`REQ_0878`). Realised as
   ``crates/taktora-connector-ui-tests/tests/derive_viewmodel.rs``.

.. test:: Command derive: CommandParams, idempotent flag and schema
   :id: TEST_0874
   :status: implemented
   :verifies: REQ_0866, REQ_0868, REQ_0878

   ``#[derive(CommandParams)]`` with and without ``#[command(idempotent)]``,
   asserting the emitted ``CommandSchema`` carries the parameter field
   descriptors, the command ``Kind``, the ``CanExecute`` kind
   (:need:`REQ_0866`), and the idempotent flag (:need:`REQ_0868`) into the
   manifest contribution (:need:`REQ_0878`). Realised as
   ``crates/taktora-connector-ui-tests/tests/derive_command.rs``.

.. test:: Non-POD ViewModel fields rejected at compile time
   :id: TEST_0875
   :status: implemented
   :verifies: REQ_0858

   ``trybuild`` compile-fail corpus asserting that heap-backed
   (``Vec``), generic, and serde-renamed fields produce a clear
   ``compile_error!`` rather than mis-encoding, and that a nested-struct
   field lands on the purposeful "not yet supported" diagnostic — the
   deferred slice of :need:`REQ_0858`. Realised as
   ``crates/taktora-connector-ui-tests/tests/trybuild.rs`` driving
   ``tests/ui/reject_vec.rs``, ``reject_nested_struct.rs``,
   ``reject_generic.rs``, ``reject_serde_rename_container.rs``, and
   ``reject_serde_rename_field.rs`` against their ``.stderr`` snapshots.

.. test:: Server-side Property seqlock RT-update handle
   :id: TEST_0876
   :status: implemented
   :verifies: REQ_0860

   Drives the server-side ``Property<V>`` RT-update handle and its
   clone-able pump-side ``PropertyReader<V>``, asserting that an update
   writes the POD struct into the per-ViewModel seqlock cell with no heap
   allocation / iceoryx2 call / codec invocation on that path, and that a
   concurrent pump-side read is torn-read-safe (seqlock retry). Realised as
   ``crates/taktora-connector-ui-tests/tests/property.rs``.

.. test:: Publish-plane iceoryx2 round-trip: cadence, coalescing, zero-subscriber skip
   :id: TEST_0877
   :status: implemented
   :verifies: REQ_0856, REQ_0861, REQ_0862

   Stands up the production ``IoxVmPublisher`` and the ``Pump`` against a
   real iceoryx2 service: asserts a ViewModel is published as one
   latest-value struct-per-service (:need:`REQ_0856`), that the non-RT
   pump snapshots / JSON-encodes / publishes at its cadence coalescing
   intermediate values (:need:`REQ_0861`), and that a zero-subscriber
   ViewModel is skipped and resumes once a subscriber attaches
   (:need:`REQ_0862`). Realised as
   ``crates/taktora-connector-ui-tests/tests/pump_iox.rs``.

.. test:: Command-plane iceoryx2 round-trip: acceptance ack, dedupe, off-RT handler
   :id: TEST_0878
   :status: implemented
   :verifies: REQ_0865, REQ_0867, REQ_0870

   Drives the production ``IoxCommandTransport`` / ``CommandHandler``
   against a UI-shaped client that publishes invocations on the request
   service and reads acks on the reply service: asserts the
   acceptance-ack request-response keyed by ``correlation_id``
   (:need:`REQ_0865`), that a retry reusing a seen ``correlation_id``
   replays the cached ack without re-executing the effect
   (:need:`REQ_0867`), and that the handler runs off the RT/WaitSet
   thread (:need:`REQ_0870`). Realised as
   ``crates/taktora-connector-ui-tests/tests/command_iox.rs``.

.. test:: Command deterministic behaviour: reason codes, back-pressure, gating
   :id: TEST_0879
   :status: implemented
   :verifies: REQ_0866, REQ_0867, REQ_0869, REQ_0871

   Unit tests over ``MockCommandTransport`` exercising the deterministic
   command behaviour without shared memory: ``CanExecute=false`` gating
   replies ``Rejected { CanExecuteFalse }`` and does not enqueue the
   effect (:need:`REQ_0866`); the bounded effect channel replies
   ``Rejected { BackPressure }`` when full rather than blocking
   (:need:`REQ_0871`); an unregistered command replies
   ``Rejected { UnknownCommand }`` and the closed ``RejectedCode`` set is
   exercised (:need:`REQ_0869`); and the ``correlation_id`` LRU dedupe
   replays cached acks (:need:`REQ_0867`). Realised as the ``#[cfg(test)]``
   unit tests in ``crates/taktora-connector-ui/src/command.rs``.

.. test:: Assembled UiConnector end-to-end over iceoryx2
   :id: TEST_0880
   :status: implemented
   :verifies: REQ_0855, REQ_0863, REQ_0872, REQ_0879

   Builds a ``UiConnector`` (declaring a ViewModel, a hot scalar, and a
   command), registers it with an ``Executor`` so the pump and
   command-handler threads spawn, then plays a UI client on a separate
   iceoryx2 node in the same process: reads the single instance-namespaced
   manifest (:need:`REQ_0872`), the ViewModel, the hot scalar published on
   its own service (:need:`REQ_0863`), and the ``SystemViewModel``
   heartbeat with epoch (:need:`REQ_0879`), and round-trips a command
   confirming ``UiConnector`` satisfies the ``Connector`` contract
   (:need:`REQ_0855`). Realised as
   ``crates/taktora-connector-ui-tests/tests/connector_iox.rs``.

.. test:: Reference client end-to-end: discovery, hash validation, diffing, restart, trust
   :id: TEST_0881
   :status: implemented
   :verifies: REQ_0864, REQ_0876, REQ_0877, REQ_0880, REQ_0881, REQ_0882, REQ_0884

   Drives the reference ``Client`` (``taktora-connector-ui-client``)
   against a live ``UiConnector`` server on a separate iceoryx2 node:
   discovery via the service registry (:need:`REQ_0877`), contract-hash
   validation with read-only fallback on mismatch (:need:`REQ_0876`),
   per-field ``PropertyChanged`` diffing with per-ViewModel staleness from
   the envelope (:need:`REQ_0864`, :need:`REQ_0880`), stateless restart
   recovery via history-depth-1 redelivery (:need:`REQ_0881`), and epoch
   change triggering a manifest re-read / re-validate (:need:`REQ_0882`).
   The same test exercises the v1 trust posture (:need:`REQ_0884`): a
   distinct local process opens the connector's services and issues
   commands with no application-level authentication handshake — command
   authority is granted by OS / iceoryx2 access control alone, as recorded
   in :need:`ADR_0107`. Realised as
   ``crates/taktora-connector-ui-tests/tests/client_iox.rs``.

.. test:: Connector health reflects local publish health
   :id: TEST_0882
   :status: implemented
   :verifies: REQ_0883

   Unit tests asserting the ``UiConnector`` health state machine reports
   local publishing health — pump running, publish back-pressure or drops
   degrade it and recovery restores ``Up`` — rather than remote-peer
   liveness, and that subscriber presence/absence is not by itself a
   health fault. Realised as the ``#[cfg(test)]`` unit tests in
   ``crates/taktora-connector-ui/src/health.rs`` and the pump-health seam
   tests in ``crates/taktora-connector-ui/src/pump.rs``.

.. test:: Golden manifest and contract hash
   :id: TEST_0883
   :status: implemented
   :verifies: REQ_0857, REQ_0873, REQ_0874, REQ_0875

   Golden-fixture test asserting the contract crate's serialization still
   produces the checked-in ``tests/golden_manifest.json`` — the canonical
   wire example other languages target — and that the manifest enumerates
   every ViewModel / command with its service names, field schemas,
   signatures, kinds, and idempotent flags (:need:`REQ_0873`), carries the
   ``contract_hash`` computed over the structural contract
   (:need:`REQ_0874`), expresses the closed self-describing ``FieldType``
   schema set including nested ``Struct`` descriptors (:need:`REQ_0875`),
   and that the ``Kind`` enumeration reserves the ``Event`` variant
   (:need:`REQ_0857`, :need:`REQ_0875`). Realised as
   ``crates/taktora-connector-ui-contract/tests/golden.rs`` (with the
   ``Kind::Event`` reservation pinned by the unit tests in
   ``crates/taktora-connector-ui-contract/src/kind.rs``). The
   language-neutral reproducibility of the ``contract_hash``
   (:need:`REQ_0874`) is additionally proved from a non-Rust consumer by
   ``crates/taktora-connector-ui-contract/py/smoke.py``, a pure-stdlib
   Python script that recomputes the hash from the same checked-in golden
   manifest via the canonical algorithm and asserts a bit-for-bit match.
