Architecture decisions
======================

arc42 §5 — the accepted decisions behind the logging stack
(:need:`FEAT_0070`). Each ``arch-decision`` ``:refines:`` the parent
requirement it serves.

.. contents:: Decisions
   :local:
   :depth: 1

----

.. arch-decision:: Adopt the log crate as workspace logging facade
   :id: ADR_0087
   :status: accepted
   :refines: FEAT_0070

   **Decision.** The workspace logging facade is the ``log`` crate.

   **Forces.** taktora must speak DLT to vehicle integrators and
   plain log macros to non-vehicle integrators. ``log`` is the
   rust-native facade with the widest backend ecosystem (``log4rs``,
   ``env_logger``, ``simplelog``, ``fern``). ``tracing`` is more
   powerful but its swap story (rewriting the Subscriber) is heavier
   for the "drop in log4rs" use case the integrator audience asks
   for.

   **Consequences.** All business crates use ``log::*`` macros.
   Existing ``tracing::*`` callers (``taktora-executor-tracing``)
   continue to work via the ``tracing-log`` bridge (per
   :need:`ADR_0090`). taktora-log re-exports the ``log`` crate's
   macros so callers can depend on the facade crate alone.
   ``embassy-rs/embassy`` follows the same pattern for std targets,
   so the workspace stays idiomatic.

   **Alternatives rejected.** (a) ``tracing`` as primary facade —
   the Subscriber-swap story is heavier for the log4rs use case.
   (b) Both facades coexisting — two ways to do the same thing
   doubles the spec surface. (c) A custom ``taktora-log`` trait
   surface — every downstream backend would need to learn a
   non-standard facade.

.. arch-decision:: Pure-Rust DLT via dlt-core; no libdlt FFI
   :id: ADR_0088
   :status: accepted
   :refines: FEAT_0072

   **Decision.** The DLT backend encodes via ``esrlabs/dlt-core``
   (pure Rust, AUTOSAR R20-11 conformant). No ``libdlt`` /
   ``dlt-sys`` / ``dlt-rs`` / ``dlt_log`` in the default Cargo graph.

   **Forces.** Vehicle integrators want bit-compatible DLT. Other
   integrators want a clean Rust crate that cross-compiles without
   ``bindgen`` against a C library that is not always present.

   **Consequences.** Every consumer compiles taktora's DLT backend
   without installing ``libdlt-dev``. Cross-compilation to
   non-Linux targets (e.g. macOS dev hosts that talk to a remote
   ``dlt-daemon`` over TCP) works out of the box. The daemon client
   is taktora's responsibility — no existing pure-Rust crate
   combines ``dlt-core``'s encoder with a UDS/TCP daemon client.

   **Alternatives rejected.** (a) FFI to ``libdlt`` — adds a system
   dep on every consumer, complicates cross-compile, blocks
   non-Linux dev hosts. (b) Reuse ``rusty-projects/dlt_log`` —
   smallest implementation effort but it is FFI-based, has no
   structured-field support, and forces ``libdlt`` on every
   consumer. (c) Reuse Eclipse OpenSOVD's ``tracing-dlt`` — ties to
   ``tracing`` facade and FFI to ``libdlt`` — rejected on both
   axes.

.. arch-decision:: Two-crate split (facade vs DLT backend)
   :id: ADR_0089
   :status: accepted
   :refines: FEAT_0070

   **Decision.** Split the workspace surface into two crates —
   ``taktora-log`` (facade + ``LogSink`` trait + init + dev
   fallback + tracing bridge) and ``taktora-log-dlt`` (DLT
   backend). The DLT backend is consumable on its own without the
   facade crate.

   **Forces.** A consumer that wants only DLT (e.g. an integrator
   wiring DLT into their own non-``log`` framework) should not
   have to pay for the facade crate. A consumer that wants only
   the facade (e.g. with ``log4rs``) should not have to compile
   ``dlt-core`` and the daemon client. The two audiences are
   asymmetric and the split is cheap.

   **Consequences.** Two ``Cargo.toml`` files instead of one. Both
   crates ship with their own version cadence under
   ``release-plz``. The DLT crate is module-public so its daemon
   client (:need:`BB_0092`) is reusable standalone.

   **Alternatives rejected.** (a) One crate with a ``dlt`` cargo
   feature — feature-gating the DLT backend in the same crate as
   the facade muddies the trait surface and bloats the dependency
   graph for users with ``default-features = false``. (b) Three
   crates (facade + DLT + console) — over-decomposition; the
   console fallback is small enough to live in the facade crate.

.. arch-decision:: Bridge existing tracing emitters via tracing-log
   :id: ADR_0090
   :status: accepted
   :refines: FEAT_0078

   **Decision.** Existing ``tracing::*`` emitters
   (``taktora-executor-tracing`` and any future tracing-using
   crate) are captured via the ``tracing-log`` bridge rather than
   being rewritten to use ``log::*`` directly.

   **Forces.** ``taktora-executor-tracing`` is the executor's
   ``Observer`` impl and is referenced in the executor spec.
   Rewriting it touches a lot of existing tests and risks
   regressions for no architectural gain. The ``tracing-log``
   bridge is one ``LogTracer::init()`` call and captures every
   ``tracing::Event`` as a ``log::Record``.

   **Consequences.** Only one output pipeline, regardless of
   whether the emitter chose ``log`` or ``tracing``. No business-
   code changes required when a crate moves between the two
   macro sets. The bridge must be installed before any tracing
   subscriber is registered — otherwise the events go to the
   subscriber and bypass the bridge.

   **Alternatives rejected.** (a) Rewrite ``taktora-executor-tracing``
   to use ``log::*`` — gratuitous churn. (b) Ship two parallel
   pipelines — duplicates plumbing and confuses consumers.

.. arch-decision:: Console dev fallback when no daemon configured
   :id: ADR_0091
   :status: accepted
   :refines: FEAT_0077

   **Decision.** When ``taktora-log::init()`` finds no DLT daemon
   socket configured and no pre-existing ``log::Log``, it installs
   a human-readable console formatter that writes one line per
   record to ``stderr``.

   **Forces.** First-time local ``cargo run`` and unit tests
   should produce visible output without the integrator standing
   up a ``dlt-daemon`` first. Silent drops on no-config are a
   debugging trap.

   **Consequences.** Unit tests and local development "just work".
   The fallback is replaced by any of the other init paths —
   explicit daemon config, pre-installed ``log4rs``, or an
   integrator-supplied ``LogSink``. The fallback formatter is small
   enough to live in ``taktora-log`` without pulling extra deps.

   **Alternatives rejected.** (a) No fallback (silent drops) — a
   debugging trap; rejected. (b) Panic on no-config — surprises
   first-time users; rejected. (c) Write to a file — requires
   choosing a path; rejected as more configuration than a fallback
   should require.
