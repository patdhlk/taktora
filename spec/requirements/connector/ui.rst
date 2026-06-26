UI connector (MVVM)
===================

A fourth concrete connector instantiating the framework's contracts, aimed at
a **local human-machine interface**. It bridges a taktora-executor application
to a user-interface process running on the *same host*, over the existing
iceoryx2 envelope transport, without binding taktora to any specific UI
toolkit or runtime.

The boundary is modelled on **Microsoft-style MVVM**: taktora exposes a
**ViewModel** (observable properties + invocable commands); the UI is the
**View** (out of taktora's scope, any language/framework); taktora's domain
(PDI process data, ``CycleObservation`` telemetry, connector health, motion
state) is the **Model**. The connector is a *passive transport* — the
application owns the binding glue that maps Model state onto ViewModel
properties; optional adapter crates may later bridge standard sources
(executor telemetry, connector health) without baking those couplings into
the connector core.

The contract is **language/runtime-agnostic**: the normative boundary is a
self-describing **manifest** plus a closed, fixed-layout (POD) field type
system carried as JSON over iceoryx2, so a UI in any language with an
iceoryx2 binding (Rust, C#/.NET, C++, Python, web-via-bridge) can bind
dynamically off the manifest. A Rust reference client crate provides the
ergonomic typed MVVM consumer on top; per-language typed codegen is deferred.

This parent feature ``:satisfies:`` :need:`FEAT_0030`; its four capability
clusters — ViewModel property transport (:need:`FEAT_0093`), the command
channel (:need:`FEAT_0094`), manifest, schema and discovery
(:need:`FEAT_0095`), and liveness, lifecycle and trust (:need:`FEAT_0096`) —
each ``:satisfies:`` it, on their own pages (see the toctree).

.. feat:: UI connector (MVVM)
   :id: FEAT_0092
   :status: open
   :satisfies: FEAT_0030

   A fourth concrete connector instantiating the framework's contracts:
   an MVVM-shaped, UI-framework-agnostic bridge between a taktora-executor
   application and a human-machine-interface process on the same host.
   ``UiConnector`` implements the shared ``Connector`` trait
   (``type Routing = UiRouting``, ``type Codec = C``, ``JsonCodec`` default)
   with the MVVM primitives — **Property**, **Command**, **CanExecute** — as
   an ergonomics layer desugared onto ``ChannelWriter`` / ``ChannelReader``.
   Latest-value property semantics ride iceoryx2 publisher history; command
   request-response rides the envelope ``correlation_id``.

   The connector is a passive transport: the application declares ViewModels
   and publishes their state; the connector knows nothing of taktora's
   internal types. Domain-source adapters (executor ``Observer`` telemetry,
   connector health) are reserved for separate optional crates so the core
   stays decoupled.

   RT-safety mirrors :need:`FEAT_0038` / ``taktora-telemetry-export``: a
   producer writes a fixed-layout POD struct into a per-ViewModel seqlock
   latest-value cell on the (possibly RT) hot path with no allocation; a
   separate non-RT publisher pump snapshots, JSON-encodes off-RT, and
   publishes at a configurable UI cadence with coalescing. The deterministic
   executor is never blocked, encoded on, or back-pressured by a UI consumer.

   The normative cross-language contract is a self-describing manifest plus a
   closed POD field type system; a ``#[derive(ViewModel)]`` macro authors
   both. A Rust reference client (``taktora-connector-ui-client``) is the
   v1 reference consumer; typed codegen for other languages is deferred.

   The load-bearing architectural choices behind this feature — passive
   transport, manifest-normative language-neutral contract, desugaring onto
   the ``Connector`` trait, the seqlock + non-RT-pump RT split, and
   acceptance-ack commands — are recorded in :need:`ADR_0107`.

Crate layout
------------

The connector follows the established multi-crate convention (the name
``host`` being already taken by the composition root, the connector is named
``ui``):

* ``taktora-connector-ui-contract`` — the language-neutral schema: manifest
  types, the ``kind`` enum (``Property`` | ``Command`` | ``CanExecute``,
  reserving ``Event``), the closed field-type descriptors, ``Rejected``
  reason codes, and the contract-hash algorithm. Shared by server and client
  so they cannot disagree; its JSON *is* the cross-language spec.
* ``taktora-connector-ui`` — the server side: the ``Connector`` impl,
  ``UiRouting``, seqlock cells, the non-RT publisher pump, manifest
  publishing, and the command handler + dedupe.
* ``taktora-connector-ui-derive`` — the ``#[derive(ViewModel)]`` /
  ``#[command(idempotent)]`` proc-macros.
* ``taktora-connector-ui-client`` — the Rust reference consumer (no executor
  dependency).
* ``taktora-connector-ui-tests`` — ``publish = false`` round-trip
  integration tests.
* *(reserved)* ``taktora-connector-ui-adapters`` — optional
  telemetry/health → ViewModel bridges.

v1 validation slice
-------------------

The first slice wires the connector into the existing ``ethercat-stepper``
example: a ``StepperViewModel`` (position ``f64``, a C-like ``state`` enum, a
``can_jog`` CanExecute bool), the mandatory ``SystemViewModel`` heartbeat
(:need:`REQ_0878`), one idempotent command (``enable``) and one
non-idempotent command (``jog_relative``), and the mandatory
``SystemViewModel`` heartbeat (:need:`REQ_0879`). The reference View is a
minimal **egui** app on ``taktora-connector-ui-client``. The language-neutral
claim
is put under test in v1 by a small **Python** smoke consumer that reads the
manifest and heartbeat ViewModel off the JSON contract (falling back to a
documented contract round-trip test only if the iceoryx2 Python binding
cannot be stood up cheaply).

Deferred / anti-goals
---------------------

* **Lossless event streams** (alarms, audit/event log) are deferred to v2.
  The ``kind`` enum reserves an ``Event`` slot; v1 models active-alarm
  *display* as an ordinary ring-buffer ViewModel property (lossy by design,
  consistent with the coalesced property path).
* **App-level authentication / role separation** is out of v1 scope; trust
  is OS- and iceoryx2-mediated (:need:`REQ_0883`).
* **Per-language typed codegen** (C#/TS/C++) is deferred; v1 ships the Rust
  reference client only, all other clients bind dynamically off the manifest.

.. toctree::
   :maxdepth: 2

   ui-properties
   ui-commands
   ui-manifest
   ui-lifecycle
