Bounded global allocator — architecture
=======================================

Design notes for the ``taktora-bounded-alloc`` crate (:need:`FEAT_0040`).
Captures the design decisions, the building-block decomposition, and
the concrete implementation footprint. Test cases live in
:doc:`../verification/bounded-alloc`.

.. contents:: Sections
   :local:
   :depth: 1

----

Solution strategy
-----------------

.. arch-decision:: Compile-time caps + hand-rolled fixed-block bitmap
   :id: ADR_0012
   :status: open
   :refines: REQ_0300

   **Context.** The allocator must serve every allocation from a
   statically-sized arena (:need:`REQ_0300`). Two axes of choice:

   * *Cap configuration.* Compile-time const generics
     (``BoundedAllocator<const MAX_BLOCKS, const BLOCK_SIZE>``) vs.
     runtime-configured (``BoundedAllocator::new(max_blocks: usize,
     block_size: usize)``).
   * *Free-list management.* Hand-rolled fixed-block bitmap vs.
     pulling in an existing crate (``talc``, ``linked_list_allocator``,
     ``buddy_system_allocator``).

   **Decision.** Const generics for the caps; hand-rolled fixed-block
   bitmap for free-list management.

   **Alternatives considered.**

   * *Runtime-configured caps.* More flexible — integrators could tune
     ``MAX_BLOCKS`` / ``BLOCK_SIZE`` without recompiling
     ``taktora-bounded-alloc``. Rejected because (a) the values are
     dimensioning decisions made once per binary and rarely changed,
     (b) const generics let the bitmap word count
     (``MAX_BLOCKS / 64``) be a compile-time array length, avoiding a
     heap-allocated bitmap or an unsafe pointer-into-static dance,
     and (c) the resulting ``static BoundedAllocator<N, S> =
     BoundedAllocator::new();`` pattern is the same shape Rust's
     standard library uses for global allocators.
   * *Variable-block allocator (linked-list / TLSF).* Better
     fragmentation behaviour for mixed allocation sizes. Rejected
     because (a) the fixed-block model is the simplest deterministic
     scheme and matches the requirement's ``MAX_BLOCKS * BLOCK_SIZE``
     wording, (b) fragmentation is a non-issue when every block is
     the same size, and (c) the implementation is ~80 lines of safe
     code plus one ``unsafe`` block for the arena pointer arithmetic.

   **Consequences.**

   ✅ Zero runtime overhead beyond a single ``compare_exchange``
   per allocation.
   ✅ The arena, the bitmap, the counters, and the lock flag all
   live in one ``static`` ``BoundedAllocator<N, S>`` instance with
   no further heap dependency.
   ❌ ``MAX_BLOCKS`` and ``BLOCK_SIZE`` are baked at compile time;
   tuning them per deployment requires a recompile. Acceptable
   because the binary is the artefact being certified.
   ❌ A workload that wants to allocate one large block plus many
   small ones wastes the small-block-equivalent capacity inside
   each large slot. Document the wastage; integrators size
   ``BLOCK_SIZE`` to their largest realistic allocation.

Building blocks
---------------

.. building-block:: taktora-bounded-alloc crate
   :id: BB_0024
   :status: open
   :implements: REQ_0300, REQ_0301, REQ_0302, REQ_0303, REQ_0304
   :refines: ADR_0012

   The ``taktora-bounded-alloc`` workspace crate. Single public type
   ``BoundedAllocator<const MAX_BLOCKS: usize, const BLOCK_SIZE:
   usize>`` exposing the ``GlobalAlloc`` impl plus the
   ``lock`` / ``alloc_count`` / ``dealloc_count`` /
   ``peak_blocks_used`` API. Internal sub-components:

   * **Arena** — ``UnsafeCell<[u8; MAX_BLOCKS * BLOCK_SIZE]>``
     wrapped in ``#[repr(align(...))]`` so blocks meet
     ``BLOCK_SIZE``-aligned layouts. Lives inside the same static
     as the allocator.
   * **Free bitmap** — ``[AtomicU64; (MAX_BLOCKS + 63) / 64]``. Bit
     ``i`` = 1 means block ``i`` is free; ``compare_exchange`` on
     a word claims a block. Tail-bits past ``MAX_BLOCKS`` are kept
     zero ("not free") so the bit scan never returns them.
   * **Counters** — three ``AtomicUsize``: ``alloc_count``,
     ``dealloc_count``, ``peak_in_use``. Maintained via
     ``fetch_add`` / ``fetch_sub`` with ``Relaxed`` ordering (no
     cross-thread invariant beyond eventual visibility).
   * **Lock flag** — single ``AtomicBool`` checked at the top of
     every ``alloc``; ``lock()`` stores ``true`` (Release),
     ``alloc`` reads (Acquire) and panics if set.

   The diagram below illustrates how the four sub-components are
   arranged inside the single ``static BoundedAllocator<N, S>``
   instance and how an ``alloc`` call traverses them:

   .. mermaid::

      graph TD
          subgraph Static["static BoundedAllocator&lt;N, S&gt;"]
              Arena["Arena<br/>UnsafeCell&lt;[u8; N×S]&gt;<br/>(repr(align(S)))"]
              Bitmap["Free bitmap<br/>[AtomicU64; (N+63)/64]<br/>bit i = 1 → block i free"]
              Counters["Counters<br/>alloc_count · dealloc_count<br/>peak_in_use  (AtomicUsize)"]
              Lock["Lock flag<br/>AtomicBool<br/>(false = open)"]
          end
          Caller["caller: alloc(layout)"]
          Caller -->|"1. load(Acquire)"| Lock
          Lock -->|"true → panic"| Panic["panic (abort)"]
          Lock -->|"false → size/align check"| Check{"layout.size ≤ S?"}
          Check -->|"no → return null"| Null["null → alloc_error_handler → abort"]
          Check -->|"yes → bit-scan"| Bitmap
          Bitmap -->|"no free bit → return null"| Null
          Bitmap -->|"CAS 1→0 on bit i"| Arena
          Arena -->|"pointer to block i"| Counters
          Counters -->|"fetch_add alloc_count<br/>update peak_in_use"| Result["return ptr"]

   Lifetime contract — the entire structure is intended for
   ``static`` storage. No public ``new`` other than the ``const fn``
   used in a ``static`` initialiser.

Implementation
--------------

.. impl:: taktora-bounded-alloc crate + sample binary
   :id: IMPL_0002
   :status: open
   :implements: BB_0024
   :refines: REQ_0300, REQ_0301, REQ_0302, REQ_0303, REQ_0304

   **Workspace integration**

   * Register ``crates/taktora-bounded-alloc`` in the root
     ``Cargo.toml`` ``[workspace] members``.
   * Crate has no runtime dependencies — only ``core`` and ``std``
     (the latter used only by the sample binary and integration
     tests; ``lib.rs`` itself is ``#![no_std]``-compatible).

   **``crates/taktora-bounded-alloc/Cargo.toml``**

   * Inherits ``edition``, ``rust-version``, ``license``, etc. from
     workspace.
   * ``[profile.dev]`` and ``[profile.release]`` set ``panic =
     "abort"`` (required by :need:`REQ_0302`).
   * ``[[example]] name = "fail_closed"`` for the sample binary.

   **``crates/taktora-bounded-alloc/src/lib.rs``**

   * ``#![no_std]`` plus ``extern crate alloc`` only inside the
     ``test`` cfg.
   * ``pub struct BoundedAllocator<const MAX_BLOCKS: usize,
     const BLOCK_SIZE: usize>`` carrying ``arena``, ``bitmap``,
     ``alloc_count``, ``dealloc_count``, ``peak_in_use``,
     ``locked``.
   * ``pub const fn new() -> Self`` — usable in a ``static``
     initialiser.
   * ``unsafe impl GlobalAlloc`` providing ``alloc`` and ``dealloc``.
     ``alloc`` body — check ``locked`` (panic if true), check
     ``layout.size() > BLOCK_SIZE`` (return null), bit-scan the
     bitmap for a free word, ``compare_exchange`` to claim a bit,
     bump counters, return pointer; ``dealloc`` body — derive
     block index from pointer offset, set bit, bump
     ``dealloc_count``.
   * ``realloc`` falls through to the default ``GlobalAlloc`` impl
     (alloc-new + ``copy_nonoverlapping`` + dealloc-old).
   * ``pub fn lock(&self)``, ``pub fn is_locked(&self) -> bool``,
     ``pub fn alloc_count(&self) -> usize``,
     ``pub fn dealloc_count(&self) -> usize``,
     ``pub fn peak_blocks_used(&self) -> usize``.
   * Single ``#[allow(unsafe_code)]`` block for the arena pointer
     arithmetic, with a ``// SAFETY:`` comment citing the
     bitmap-CAS invariant (a thread that observes a ``1->0``
     transition on bit ``i`` is the unique owner of block ``i``
     until it CASs the bit back to ``1`` in ``dealloc``).

   The lock lifecycle (:need:`REQ_0302`) is one-way — once
   ``lock()`` is called there is no path back to ``Open``:

   .. mermaid::

      stateDiagram-v2
          [*] --> Open : static initialisation
          Open --> Open : alloc / dealloc (normal operation)
          Open --> Locked : lock() — stores true (Release)
          Locked --> Locked : dealloc (still permitted)
          Locked --> Abort : alloc / alloc_zeroed / realloc\n→ panic → abort

   **``crates/taktora-bounded-alloc/tests/``**

   * ``cap.rs`` — exhaustion and oversize cases (:need:`TEST_0180`).
   * ``lock.rs`` — lock then alloc panics (:need:`TEST_0182`).
   * ``counters.rs`` — counter accuracy (:need:`TEST_0183`).
   * ``thread_safety.rs`` — concurrent alloc/dealloc smoke
     (:need:`TEST_0184`).
   * Tests instantiate ``BoundedAllocator`` as a regular ``static``
     (not registered as ``#[global_allocator]``) and call its
     ``GlobalAlloc`` methods directly. This lets the test harness
     itself continue to use the system allocator for
     ``Vec``/``String``/etc.

   **``crates/taktora-bounded-alloc/examples/fail_closed.rs``**

   * Sets a ``BoundedAllocator<8, 64>`` as ``#[global_allocator]``.
   * In ``main`` allocates ``Box<[u8; 32]>`` in a loop, printing
     the running block index, until the 9th allocation triggers
     the default ``alloc_error_handler`` → ``abort``. The
     surviving 8 allocations are observable in the printed output
     before the abort; the process exits with a non-zero status
     (SIGABRT). Demonstrates :need:`REQ_0301` end-to-end.

   **``crates/taktora-executor/tests/no_alloc_dispatch.rs`` migration**

   * Replace the inline ``CountingAllocator`` (the existing
     ~70 lines covering ``GlobalAlloc``, size buckets, tracking
     flag) with a dependency on ``taktora-bounded-alloc``'s public
     counting API. Set ``MAX_BLOCKS`` deliberately high
     (~``1 << 20``) so the steady-state caps never fire during
     the differential measurement — :need:`REQ_0060` is about
     the *count*, not about provisioning.
   * Add ``taktora-bounded-alloc`` to
     ``crates/taktora-executor/Cargo.toml`` ``[dev-dependencies]``.
   * Delete the size-bucket diagnostic and the
     ``trip_first_alloc_backtrace`` test (use the new crate's
     counters; deeper triage stays a developer-side workflow,
     not a checked-in test).
