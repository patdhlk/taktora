# Implementation plan — Framework internal-fault model (FEAT_0024)

**Spec source of truth:** `spec/requirements/plc-runtime.rst` (FEAT_0024, REQ_0123/0124/0125),
`spec/architecture/plc-runtime.rst` (ADR_0065, BB_0094, IMPL_0085/0086),
`spec/safety/aou.rst` (AOU_0016), `spec/verification/plc-runtime.rst` (TEST_0823/0824/0825).

**Crate:** `taktora-executor` (std; uses `std::sync::Mutex`, `std::thread`, `std::panic`).

---

## 1. Goal

Turn the cyclic path's panic handling from *"swallow everything"* into a two-class model:

- **User-item panic** → caught at the inner layer, converted to `PanickedTask` → `Faulted`
  (REQ_0124). **Already works** (`run_item_catch_unwind`, `executor.rs:1538`); this work
  *retro-documents* and *regression-guards* it.
- **Framework-invariant panic** → reaches the outer (framework) boundary, where it must
  **fail fast**: run a best-effort user fatal handler, then `std::process::abort()`
  (REQ_0123 + REQ_0125). **This is the new behavior.**

The outer boundary today (`pool.rs:167`, `:175`, `:211`) does `let _ = catch_unwind(...)` —
it *swallows* infra panics, which can deadlock the executor with frozen outputs and no fault
surfaced (ADR_0065 Context). That swallow is the defect being fixed.

Output safe-state on abort is **out of taktora's hands by design**: `abort()` runs no
destructors, so the fieldbus SM watchdog drives outputs safe (AOU_0016). Not implemented here.

---

## 2. The boundary invariant (why this is sound)

Because user-item panics are *already* converted to `Err` by the inner `catch_unwind`
**below** the outer boundary, the only panics that can reach the outer boundary are
framework-internal. So: **any panic at the outer boundary ⇒ broken invariant ⇒ abort.**
No classification logic is needed at the boundary — its mere reach *is* the classification.

Outer boundary lives at every runtime-thread top:
1. Pool **worker loop** — `pool.rs:166-169` (`Job::Owned`) and `:170-179` (`Job::Borrowed`).
2. Pool **inline-submit** path — `pool.rs:210-213` and `:239-245`.
3. Executor **dispatch-thread run loop** — wraps the per-iteration dispatch in
   `executor.rs` (`dispatch_loop` / `run_once_borrowed` call site).

---

## 3. API surface (new)

```rust
/// Why the runtime is about to abort. Passed to the fatal handler.
#[non_exhaustive]
pub struct FatalContext {
    /// Best-effort message extracted from the panic payload.
    pub cause: String,
    /// Which runtime boundary caught it.
    pub site: FatalSite,
}

#[non_exhaustive]
pub enum FatalSite { PoolWorker, InlineSubmit, ExecutorRunLoop }

pub type FatalHandler = Arc<dyn Fn(&FatalContext) + Send + Sync + 'static>;

impl ExecutorBuilder {
    /// Register a best-effort, time-bounded last-gasp invoked once on the
    /// fail-fast path immediately before `std::process::abort()`.
    ///
    /// Contract (REQ_0125): runs over known-unsound executor state — MUST NOT
    /// touch executor internals; a panic inside it routes straight to abort.
    pub fn on_fatal(self, handler: impl Fn(&FatalContext) + Send + Sync + 'static) -> Self;
}
```

Default handler = no-op. Stored as `Option<FatalHandler>` on the builder; resolved to a
no-op `Arc` at `build()`.

### Terminal-action seam (for testability)

The terminal step is abstracted so TEST_0823 can observe the boundary in-process without
the test harness aborting:

```rust
// Crate-internal. Production wiring => abort. Tests can swap the terminal.
pub(crate) struct FatalDispatch {
    handler: FatalHandler,
    terminal: Arc<dyn Fn(&FatalContext) + Send + Sync>, // default: process::abort wrapper
}

impl FatalDispatch {
    /// Run handler (catch-guarded so a handler panic still terminates), then terminal.
    pub(crate) fn fire(&self, ctx: &FatalContext) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.handler)(ctx)));
        (self.terminal)(ctx); // production: aborts (diverges); test: records + returns
    }
}
```

- **Production** `terminal` = `|_| std::process::abort()` (diverges; the worker loop / run
  loop never observe a return).
- **Test** `terminal` = records `ctx` into a shared cell and returns; the boundary then
  continues/breaks. Wired only via a `#[cfg(test)]` constructor — never reachable in release.

`FatalDispatch` is `Arc`-cloned into `Pool::new` (so worker threads hold it) and kept on
`Executor` (for the run-loop boundary).

### Shared payload→message helper

Factor the downcast in `run_item_catch_unwind` (`executor.rs:1540-1546`) into:

```rust
pub(crate) fn panic_payload_message(payload: &Box<dyn Any + Send>) -> String;
```

Reused by both the item layer and `FatalContext::cause`.

---

## 4. Commits (TDD: test first within each)

### Commit 1 — payload message helper (pure refactor)
- Extract `panic_payload_message` from `run_item_catch_unwind`; call it from there.
- Test: `&str`, `String`, and unknown payloads map to expected strings.
- No behavior change. Green = existing item-panic tests still pass.

### Commit 2 — `FatalContext` / `FatalSite` / `on_fatal` builder + `FatalDispatch`
- Add the types and the builder setter; thread `FatalDispatch` (`Arc`) into `Pool::new`
  and onto `Executor`. Default no-op handler + abort terminal.
- Test: builder round-trips a handler; `FatalDispatch::fire` with a **test terminal** runs
  the handler then the terminal, in order; a handler that panics still reaches the terminal
  (catch-guard works).
- Boundary not yet rewired — `let _ = catch_unwind` still in place. Green = no behavior change.

### Commit 3 — rewire the outer boundary (the core change) → REQ_0123, IMPL_0085, BB_0094
- Replace `let _ = catch_unwind(f)` at `pool.rs:167/175/211/241` with:
  ```rust
  if let Err(p) = catch_unwind(AssertUnwindSafe(f)) {
      fatal.fire(&FatalContext { cause: panic_payload_message(&p), site: FatalSite::PoolWorker /* or InlineSubmit */ });
  }
  ```
- Wrap the executor dispatch-thread per-iteration call (`run_once_borrowed` invocation) in
  `catch_unwind`; on `Err` → `fatal.fire(ExecutorRunLoop)`.
- **TEST_0823** (in-process): register a recording handler + test terminal; fire a synthetic
  non-item panic via `pool.submit(|| panic!("synthetic infra panic"))` through inline +
  threaded modes, and via a synthetic panic on the run-loop path; assert the recorded
  `FatalContext.cause` / `.site` for each.
- **TEST_0824** (subprocess): a `#[test]` that re-execs itself (env-var-guarded child) with
  the **default** terminal, triggers a boundary panic, and asserts the child died via
  `SIGABRT` (`WIFSIGNALED` + `SIGABRT`; on Unix via `std::os::unix::process::ExitStatusExt`).

### Commit 4 — no-panic classification gate → IMPL_0085 (audit half)
- Add function-scoped `#[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` to
  the cyclic-path fns: `graph::run_once_borrowed`, the `prepare_dispatch` vertex closure,
  `graph::dispatch_vertex`, the pool worker loop, `pool::submit_borrowed`.
- Annotate each surviving intentional fail-fast site with
  `#[allow(clippy::expect_used)] // fail-fast: <invariant>` — e.g.
  `ready_ring.push().expect()` (`graph.rs:489`), the `done_cv` / `first_err` /
  `iter_err` `lock().unwrap()` sites, `wait_timeout(...).unwrap()` (`graph.rs:551`).
- Convert any site that turns out to be a *recoverable* condition (none expected) to `Err`.
- Green = `cargo clippy -p taktora-executor -- -D warnings` passes.

### Commit 5 — retro-document item-panic containment → REQ_0124, IMPL_0086, TEST_0825
- No production change. Add **TEST_0825**: a user item panicking in `execute` is caught and
  surfaced via `on_app_error` as a `PanickedTask`, leaves the task `Running` (containment is
  NOT a `Faulted` transition — `Faulted` is reserved for deadline breaches, REQ_0070), leaves
  siblings running, and does **not** invoke the fatal handler nor abort.
- This is the regression guard that the inner layer never escalates to the fail-fast path.

---

## 5. Verification checklist

- `cargo test -p taktora-executor` (+ `-tests` crate): TEST_0823/0824/0825 green.
- `cargo clippy -p taktora-executor -- -D warnings`: cyclic-path gate green.
- Existing allocation-free tests (TEST_0194, TEST_0821) still green — the boundary adds no
  steady-state allocation (the `FatalContext`/`String` is built only on the panic path).
- `cd spec && make strict` — already green with the new needs.

---

## 6. Out of scope / deferred (tracked, not in this change)

- **SM-watchdog modeling + ≤ FTTI/2 enforcement** (AOU_0016, ADR_0065 "Alternatives"):
  parse per-SM watchdog enable/timeout in `taktora-ethercat-esi`, surface into
  `taktora-ethercat-netcfg`, and validate the bound at config time. Separate dependent slice
  across two crates. File as its own issue/feature; this change only *documents* the
  assumption.

---

## 7. Risks & notes

- **`AssertUnwindSafe`**: the boundary closures capture `&mut` state across `catch_unwind`.
  This is sound here because on `Err` we abort (production) — we never resume use of
  possibly-inconsistent captured state. The test terminal returns, but test closures don't
  hold poisoned invariants. Document with a `// SAFETY/abort:` comment.
- **Threaded abort**: `abort()` fires on whichever worker thread caught the panic; it kills
  the whole process — correct. `tracker.complete()` after the boundary is unreachable on the
  abort path (fine).
- **No global panic hook**: deliberately not used — a global hook cannot distinguish the
  inner (caught, must-not-abort) item panic from an infra panic, since the hook runs for
  *both* before unwinding. The layered `catch_unwind` seam is the only correct discriminator.
- **`panic = "unwind"` stays global**: a `Cargo.toml` `panic = "abort"` profile would break
  the inner catch-and-fault path (REQ_0124) — must not be set. Consider a CI assertion that
  no profile sets `panic = "abort"`.
