Workspace unsafe-code gate
===========================

Standing tooling that enforces a ``#![forbid(unsafe_code)]`` boundary on
selected crates (initially ``taktora-executor``) with ``cargo-geiger``
verification, so the surface area of code requiring manual memory-safety
audit is auditable and cannot expand without deliberate opt-out.

.. feat:: Workspace unsafe-code gate
   :id: FEAT_0201
   :status: implemented

   **Motivation.** Unsafe Rust — raw pointers, FFI, ``unsafe impl Send``,
   compiler-unchecked invariants — trades type-system safety for control,
   and every ``unsafe`` block is a manual correctness obligation. For a
   project building a safety argument, the unsafe surface must be (a)
   minimised to what zero-alloc steady-state dispatch, OS thread policy,
   and IPC primitives genuinely require, (b) isolated in an audited
   boundary crate where every block carries a ``// SAFETY:`` rationale,
   and (c) enforced so a refactor cannot silently add unsafe to a crate
   that forbids it. Today ``taktora-executor`` scatters unsafe across its
   modules with no enforcement; a contributor could add an unchecked slice
   or transmute to the hot path without any gate rejecting it.

   **Scope.** A new published crate ``taktora-executor-sys`` holds the
   audited unsafe boundary with sound safe abstractions: (a) ``dispatch``
   — ``ExclusiveCell<T: ?Sized>`` (AtomicBool busy flag + UnsafeCell)
   providing CAS-acquired exclusive access via ``try_with(|&mut T| ..)``
   with panic-safe drop-guard release; the executor uses
   ``Arc<ExclusiveCell<...>>`` for shared jobs/vertices/chains instead of
   raw pointers, preserving zero-alloc steady-state (per-dispatch cost:
   one Arc refcount + one CAS, no allocation). (b) ``ports`` — iceoryx2
   IPC port ``Send`` / ``Send+Sync`` wrappers (SendPublisher,
   SendSubscriber, SendNotifier, SharedNotifier, SendServer, SendClient,
   SendPendingRequest, SendListener) exposing only the methods the
   executor calls; soundness rests on iceoryx2's SingleThreaded Rc arc
   policy being touched only at construction (documented assumption). (c)
   ``os`` (Linux-only) — ``TimerFd`` (iceoryx2-bb-posix fd traits),
   ``set_current_thread_timer_slack_ns``, ``set_current_thread_sched_fifo``.
   Dispatch-thread state (task table, cycle stats, fault atomic, start
   time, stop listener) is reached via ordinary lifetime-checked borrows
   (``DispatchPass<'a>``); WaitSet attachments no longer erase lifetimes.
   Every unsafe block carries ``// SAFETY:`` comments and clippy
   ``undocumented_unsafe_blocks = "deny"`` + ``unsafe_op_in_unsafe_fn =
   "deny"`` enforce them. ``taktora-executor`` declares
   ``#![forbid(unsafe_code)]`` in ``lib.rs`` and consumes the sys crate.
   The gate is ``scripts/check-unsafe.sh`` wrapping ``cargo-geiger``
   (LoC-counting unsafe scanner, v0.13): fails hard if any crate in the
   ``FORBID_CRATES`` array (initially ``taktora-executor``) has
   ``forbids_unsafe == false`` or non-zero used unsafe; emits an
   informational per-crate report of all workspace members (incl.
   connectors, not gated) to ``target/geiger/report.md`` + JSON when
   ``GEIGER_REPORT=1`` is set, appended to the GitHub job summary. The
   script self-skips with an install hint when ``cargo-geiger`` or
   ``jq`` are missing. CI job ``unsafe`` in ``.github/workflows/ci.yml``
   runs it with ``GEIGER_REPORT=1`` and uploads
   ``target/geiger/`` as artifact ``geiger-report``; pre-push hook
   ``unsafe-gate``; ``CONTRIBUTING.md`` "Unsafe code (cargo-geiger)"
   section.

   **Non-goals.** Gating connector crates (explicitly deferred — they
   consume third-party protocol libraries with their own unsafe);
   eliminating unsafe from the executor via ``Arc`` / ``Mutex`` / rustix
   redesign (changes hot-path dispatch / RT characteristics,
   incompatible with :need:`REQ_0104` zero-alloc steady-state); a
   ratchet-only gate that reports but does not enforce forbid (does not
   achieve the compile-time hard boundary the requirement needs).

.. req:: Sys crate for audited unsafe boundary
   :id: REQ_1208
   :status: implemented
   :satisfies: FEAT_0201
   :links: IMPL_0094, TEST_0987

   The workspace shall provide ``taktora-executor-sys`` as a published
   crate holding the audited unsafe code extracted from
   ``taktora-executor`` with sound safe abstractions: ``dispatch``
   (``ExclusiveCell<T>`` CAS-based exclusive access primitive), ``ports``
   (iceoryx2 IPC port ``Send`` / ``Send+Sync`` wrappers), and ``os``
   (Linux-only timerfd / thread policy FFI).

.. req:: Every unsafe block carries a SAFETY comment
   :id: REQ_1209
   :status: implemented
   :satisfies: FEAT_0201
   :links: IMPL_0094, TEST_0987

   Every ``unsafe`` block and ``unsafe impl`` in ``taktora-executor-sys``
   shall carry a ``// SAFETY:`` comment stating the invariants that make
   the operation sound. Clippy ``undocumented_unsafe_blocks = "deny"``
   and ``unsafe_op_in_unsafe_fn = "deny"`` shall enforce this at build
   time.

.. req:: taktora-executor forbids unsafe code
   :id: REQ_1210
   :status: implemented
   :satisfies: FEAT_0201
   :links: IMPL_0094, TEST_0987

   ``crates/taktora-executor/src/lib.rs`` shall declare
   ``#![forbid(unsafe_code)]``, making any unsafe use — block, fn, impl,
   or allow-attribute opt-out — a hard compile error in that crate's
   modules.

.. req:: Unsafe-gate entrypoint script
   :id: REQ_1211
   :status: implemented
   :satisfies: FEAT_0201
   :links: IMPL_0094, TEST_0988

   The repository shall provide ``scripts/check-unsafe.sh`` that verifies
   unsafe boundaries using ``cargo-geiger`` and enforces that each crate
   in the ``FORBID_CRATES`` array has ``forbids_unsafe == true`` and zero
   used unsafe functions/expressions/impls/traits.

.. req:: Per-crate geiger report
   :id: REQ_1212
   :status: implemented
   :satisfies: FEAT_0201
   :links: IMPL_0094, TEST_0988

   When ``GEIGER_REPORT=1`` is set, the unsafe-gate script shall emit an
   informational report of all workspace members (unsafe counts, LoC) to
   ``target/geiger/report.md`` (markdown) and ``target/geiger/report.json``
   (structured), not gated — connectors and dependencies are observable
   but not enforced.

.. req:: Missing-tool diagnostic
   :id: REQ_1213
   :status: implemented
   :satisfies: FEAT_0201
   :links: IMPL_0094, TEST_0989

   When ``cargo-geiger`` or ``jq`` are not installed, the unsafe-gate
   script shall exit zero with an actionable install hint instead of
   failing mid-run, so contributor builds degrade gracefully while CI
   (which installs the tools) gates strictly.

.. req:: Contributor documentation
   :id: REQ_1214
   :status: implemented
   :satisfies: FEAT_0201
   :links: IMPL_0094, TEST_0990

   ``CONTRIBUTING.md`` shall document the unsafe-code gate: the forbid
   policy, the sys crate boundary, how to run the gate locally, and where
   the reports land.

.. req:: CI enforces the unsafe gate
   :id: REQ_1215
   :status: implemented
   :satisfies: FEAT_0201
   :links: IMPL_0094, TEST_0988

   CI shall enforce the unsafe gate on code-changing pull requests and
   pushes to ``main`` by running the same entrypoint as local development
   (``scripts/check-unsafe.sh``), emitting the full report to the GitHub
   job summary, and uploading ``target/geiger/`` as a workflow artifact.
