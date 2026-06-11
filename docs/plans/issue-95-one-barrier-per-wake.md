# Implementation plan — One barrier + one telemetry fold per wake (issue #95)

**Spec source of truth:** `spec/requirements/*` REQ_0854 (per-phase dispatch dedup),
REQ_0268/ADR_0100 (absolute-grid dispatch), REQ_0060 (zero-alloc steady state),
REQ_0107 + FEAT_0038 (`cycle_index` join key), REQ_0123 (framework-fault fail-fast).

**Crate:** `taktora-executor` (all changes in `src/executor.rs`; one new test file).

**Blocked by:** #93 (per-phase dedup guard) — already merged; `dispatch_task`'s
`pending_cycle.is_some()` guard (`executor.rs:1825`) is the aliasing prerequisite.

---

## 1. Goal

Collapse the dispatch path from **one `barrier()` + one O(task_count) fold per fired
attachment, plus a separate pair in the grid pass** to **exactly one `barrier()` + one
O(task_count) fold per wake**, covering event-dispatched and grid-dispatched tasks
together.

Each eliminated `barrier()` is a potential condvar park/wake round-trip on the WaitSet
thread; merging also lets all fired tasks (event + cyclic) run concurrently across the pool
instead of task *k+1* waiting on task *k*'s barrier.

### Named behavior change (D1 — not a pure refactor)

#95 *activates* the #93 dedup guard for real multi-listener event tasks. A task with two
listeners both hot in one wake goes from **dispatched once per fired listener** (today: each
`process_attachment` self-barriers, clearing `pending_cycle`, so the second listener
re-dispatches) to **dispatched once per wake**, with the item's `take()` loop draining all
ready listeners in that single run. This is the intended, more-correct contract — it is
documented and pinned by a new test, not smuggled in.

---

## 2. The soundness invariant (why this is sound)

`validate_decls` (`executor.rs:2298`) forbids interval+listener on one task, so **every task
is event-XOR-cyclic**: a task is dispatched by *either* the WaitSet callback *or* the grid
pass in a given wake, never both. Combined with the #93 dedup guard (re-touching an
in-flight task is impossible until the fold), the single fold over all task indices records
each dispatched task exactly once.

While task A's borrowed job is in flight, the WaitSet thread only forms `&mut TaskEntry` for
*other* tasks; A's in-flight job holds pointers into A's entry only. This is the same
aliasing discipline `run_grid_cyclic_pass` already exercises (dispatch every due cyclic task
before its single barrier) — extended to span the event callback too, not invented.

The **one** `barrier_and_record` runs **unconditionally** after the wait/grid pass and
**before** `after_callback` can break the loop, so no in-flight borrowed job outlives
`tasks_ptr` exclusivity at loop exit / `Executor` drop.

### 2a. Dedup-token lifetime (latent-bug fix, predates #95)

`pending_cycle` doubles as the #93 per-phase dedup token. Its intended lifetime is **one
`dispatch_loop` invocation** — set at dispatch, `take()`n by the fold. But a test-terminal
bail (`guard_or_fatal` → `None` → `break Ok(())`) can leave it `Some` if the panic landed
after a mark but before the fold. The executor is `&mut self` and `self.tasks` persists, so a
subsequent `run_*` call enters a fresh `dispatch_loop` where `dispatch_task`'s
`if task.pending_cycle.is_some() { return }` (`executor.rs:1825`) **silently swallows that
task's first dispatch**. D4's bail-path `pool.barrier()` drains in-flight *jobs* but does not
clear *tokens*, so it does not close this.

This is **latent on `main` since #93** (a mid-fold observer panic leaves later tasks' stashes
set) and **#95 widens it considerably** — marks now accumulate across the entire callback
phase with no intervening barrier, so any infrastructure panic during the wait can strand a
whole wake's worth of tokens.

**Fix at the source:** an O(task_count), alloc-free sweep at `dispatch_loop` entry clearing
`pending_cycle` / `pending_skipped` / `pending_late` on every `TaskEntry` (the legitimately
cross-cycle grid state — `grid_epoch` / `grid_slot` / `last_dispatch` — is untouched). One
scan per `run_*` call, not per wake. This makes the token's "within one invocation" lifetime
hold **by construction**, covering every exit path including ones not yet written.

---

## 3. Changes, by site

> Ordering: §3d (REQ_0123 boundary) and §3e (token sweep) are **standalone latent-`main`
> bug fixes** that ship in a **separate precursor PR**, not on the #95 branch (see §4 for the
> rationale). #95 rebases onto that PR once merged and contains only §3a–3c.

### 3a. `process_attachment` → mark-and-submit only (`executor.rs:1883`)

- **Delete** the `self.barrier_and_record();` call at `:1909`.
- Body becomes: drain stop notifications → `map.resolve` → `dispatch_task` if not `IGNORE`
  → `return CallbackProgression::Continue`.
- Update the doc comment to state the barrier is deferred to `dispatch_loop`'s single
  per-wake `barrier_and_record`.

### 3b. `run_grid_cyclic_pass` → borrow, no internal barrier (`executor.rs:1582`)

- Change signature `mut pass: DispatchPass<'_,'_,'_>` → `pass: &mut DispatchPass<'_,'_,'_>`
  (D2: borrow, not move — `dispatch_loop` keeps ownership for the barrier).
- **Delete** the trailing `pass.barrier_and_record();` at `:1612`.
- **Keep** (D5) all four early-return gates (`!ticked` / `stopping` / `!= Grid` /
  `cyclic_task_indices.is_empty()`) and the `cb_result` + `stop_flag` params. The
  stop-suppression is load-bearing: a stop/interrupt wake must emit **no** cyclic
  `pending_cycle`, or a `stop()` injects one extra `CycleObservation` and desyncs the
  FEAT_0038 `cycle_index` join key.
- Update the doc comment: this pass now *only* dispatches; the barrier moved to the caller.

### 3c. `dispatch_loop` — single unconditional barrier inside the boundary (`:1426–1473`)

The post-wait `guard_or_fatal` boundary is already in place from §3d (precursor). This
perf commit (a) strips the per-attachment + grid-internal barriers, (b) switches
`run_grid_cyclic_pass` to `&mut`, and (c) adds the **one** `cpass.barrier_and_record()`
inside that boundary. End state of the region (D2 + D3 combined):

```rust
// First guard_or_fatal (wait + mark-and-submit callbacks) — unchanged body,
// but the None bail now drains in-flight jobs (D4):
let Some(cb_result) = guard_or_fatal(&self.fatal_dispatch, FatalSite::ExecutorRunLoop, || {
    let mut pass = DispatchPass { /* event pass, unchanged */ };
    let timeout = /* unchanged */;
    waitset.wait_and_process_once_with_timeout(
        |id| pass.process_attachment(&id, &mut attachment_map), timeout)
}) else {
    self.pool.barrier();            // D4: defensive drain before teardown
    break Ok(());
};

// Second guard_or_fatal (D3): wraps ticked + grid pass + the single barrier,
// restoring REQ_0123 abort-on-observer-panic for BOTH populations.
let Some(()) = guard_or_fatal(&self.fatal_dispatch, FatalSite::ExecutorRunLoop, || {
    #[cfg(target_os = "linux")]
    let ticked = master_timer.as_ref().is_some_and(|tf| tf.drain() > 0);
    #[cfg(not(target_os = "linux"))]
    let ticked = true;

    let mut cpass = DispatchPass { /* same borrows as before */ };
    run_grid_cyclic_pass(
        &mut cpass, ticked, dispatch_mode, &stop_flag, cb_result,
        &mut grid, self.cyclic_clock.now_nanos(),
        &cyclic_task_indices, &mut due_cyclic,
    );
    cpass.barrier_and_record();      // D2: the ONE barrier+fold per wake,
                                     // unconditional, covers event + grid
}) else {
    self.pool.barrier();            // D4: defensive drain before teardown
    break Ok(());
};

match self.after_callback(cb_result, mode, &iterations_done, &stop_flag) { /* unchanged */ }
```

Notes:
- `cb_result` is `Copy` (used today at both `:1459` and `:1469`); capturing it into the
  second closure and re-reading it in `after_callback` stays legal. **Verify at compile
  time** — this is the riskiest borrow.
- The second closure mirrors the first's disjoint-field capture pattern
  (`self.cyclic_clock` / `master_timer` locals while `guard_or_fatal` borrows
  `&self.fatal_dispatch`), already proven by the first `guard_or_fatal`.
- `barrier_and_record` still folds `0..task_count` via `pending_cycle.take()`; event tasks'
  stashes set inside the first closure persist on the `TaskEntry`s and are folded here even
  though a *different* `DispatchPass` value set them (same `tasks_ptr`). Event tasks
  early-return in `record_cycle_for` (no `scan_period`), but the `take()` still clears their
  dedup token for the next wake. ✓

### 3d. (PRECURSOR) Wrap post-wait cyclic dispatch+fold in the framework-fault boundary (REQ_0123)

Standalone soundness fix, latent on `main`. Today `run_grid_cyclic_pass` (and its
`barrier_and_record` → `record_cycle_for` → `observer.on_cycle_stats`) runs **outside** the
`guard_or_fatal` that closes at the wait. So an observer panic in the **cyclic** fold escapes
the REQ_0123 boundary entirely — no fatal handler, no `abort()`, a raw unwind out of
`dispatch_loop`. (Event-task folds are inside the boundary today only because
`process_attachment` self-barriers; #95 moves them out too, which is *why* the boundary must
exist before the perf change.)

Fix: wrap the existing post-wait region (`ticked` compute + `run_grid_cyclic_pass`) in a
second `guard_or_fatal(&self.fatal_dispatch, FatalSite::ExecutorRunLoop, …)`, `None` → bail.
No perf change in this commit — `run_grid_cyclic_pass` still owns its internal barrier here;
§3c later moves it. This commit alone closes the cyclic-fold escape on `main`.

### 3e. (PRECURSOR) Clear per-wake dedup tokens at `dispatch_loop` entry (REQ_0854)

Standalone latent-bug fix (see §2a). Immediately after the WaitSet/guards are built and
before the run `loop`, sweep every `TaskEntry` clearing `pending_cycle = None`,
`pending_skipped = 0`, `pending_late = None`. Alloc-free, O(task_count), once per `run_*`
call. Lands before §3a–3c because #95 widens the window it closes.

### 3f. `iter_err` / error ordering (no code change, verify)

The single barrier runs before `after_callback`, so all pool-job errors are flushed to
`iter_err` before `after_callback` reads it (`:1520`). Event-task errors that were flushed by
`process_attachment`'s barrier today are now flushed by the single barrier — still before
`after_callback`. Equivalent.

---

## 4. PR & commit structure

**Two PRs.** The soundness fixes ship *first and separately* — they fix bugs that exist on
`main` today and must not be coupled to #95's conditional merge gate.

### PR A — precursor: latent dispatch-soundness fixes (own issues, own release note)

Independent of #95. Each commit references its own issue; the REQ_0123 fix is a **production
behavior change** (observer panic in the cyclic fold: raw unwind → abort-via-fatal-handler)
and earns a release note that is *not* subordinate to "perf: one barrier per wake."

1. **`fix(executor): clear per-wake dedup tokens at dispatch_loop entry (REQ_0854)`** — §3e +
   re-entry test (§5.3). Closes the stale-`pending_cycle` swallow. `Closes #102` (origin: #93).
2. **`fix(executor): route cyclic telemetry fold through the framework-fault boundary
   (REQ_0123)`** — §3d + cyclic-observer-panic test (§5.4) + REQ_0123 discrepancy note (§6).
   Closes the cyclic-fold escape. `Closes #103`. No perf change.

Rationale for splitting (not commits 1–2 of #95):
- **Decisive:** #95's Pi A/B is a *conditional* merge gate — a no-win A/B kills the branch.
  Buried as commits, a dead #95 branch would force cherry-picking the fixes back out; landed
  first, the worst case is "`main` is simply correct and #95 dies cleanly."
- **Discrepancy/release hygiene:** distinct origins (#93 vs predates-both), distinct test
  linkage, trivially backportable to the published 0.2.x line; a `!`-bearing branch is not.
- **Sharper #95:** the RED pinning test then fails against an already-correct `main`, so it
  isolates exactly the D1 delta and nothing else.

### PR B — #95 itself (rebased onto PR A)

3. **`test(executor): pin multi-listener once-per-wake dispatch (#95, D1)`** — §5.1, RED
   against a now-correct baseline; fails until commit 4.
4. **`perf(executor)!: one barrier + one fold per wake (#95)`** — §3a–3c. Pinning test GREEN;
   telemetry / no-alloc / seam tests stay green. `!` marks the D1 contract change.
5. **`docs(spec): note once-per-wake-drain contract on REQ_0854`** — §6 (REQ_0854 delta only;
   the REQ_0123 note ships with PR A).
6. **`docs(plan): record issue #95 plan`** — this file.

---

## 5. Tests

### 5.1 New pinning test — `tests/multi_listener_one_dispatch.rs` (D1)

Model on `run_loop.rs::subscriber_trigger_dispatches_task` (`:84`):

- Two channels `A`, `B`; one subscriber each.
- One item declaring **both** triggers: `d.subscriber(&sub_a).subscriber(&sub_b)`.
- An `AtomicUsize` execute counter in the item body (also `take()` both subscribers to model
  drain-all).
- Publish to **both** channels **before** `run_n(1)` so both listeners are latched before the
  first wait → one wait processes both attachments → callback fires twice for the **same**
  task index.
- **Assert: counter == 1** (the merged contract). On pre-#95 code this is 2 — the test is a
  sharp discriminator.
- `worker_threads(0)` for determinism; event tasks attach identically in Grid and Legacy, so
  no `DispatchMode` pin needed.

### 5.2 Run unchanged (regression guards)

- `MockClock` telemetry tests — cyclic `cycle_index` advances **once per wake-phase per
  task** (event-XOR-cyclic makes this structural).
- `executor.rs::dispatch_twice_one_barrier` seam (`:2777`) — still two dispatches + one
  barrier; the dedup guard still skips the second submit.
- `taktora-executor-tests/tests/no_alloc_dispatch.rs` — the Grid-path cyclic chain already
  exercises the merged barrier+fold; `task.id.clone()` is proven refcount-only. No new
  allocation source (record count per cyclic task per wake is still exactly one). The §3e
  entry sweep runs once per `run_*` (outside the differential window), so it adds 0 to the
  per-iter measurement.

### 5.3 Re-entry after token-stranding bail (§3e regression)

White-box test (or `#[cfg(test)]` seam): drive a test-terminal bail **mid-wake** with a task
whose `pending_cycle` is set but not yet folded, then call `run_n` again and assert the task
**still dispatches** (its first dispatch is not swallowed by a stale token). Fails before §3e,
passes after.

### 5.4 Cyclic-observer-panic hits the fatal boundary (§3d regression)

Assert that a panic from `Observer::on_cycle_stats` on a **cyclic** task's fold routes to the
`FatalSite::ExecutorRunLoop` boundary (fatal-dispatch invoked / `None`-bail under the test
terminal), not a raw unwind out of `dispatch_loop`. Mirror an existing REQ_0123 fatal-path
test for the event side. Fails before §3d, passes after.

---

## 6. Spec delta (D6)

Two distinct, separately-attributed deltas:

- **REQ_0854** (per-phase dispatch dedup): with the per-attachment barrier removed, the dedup
  guard now governs observable dispatch multiplicity — **a task is dispatched at most once per
  wake; a multi-listener task's item drains all ready listeners in that single run.** Add the
  dedup-token-lifetime invariant (§2a): tokens are cleared at `dispatch_loop` entry, so their
  lifetime is one invocation by construction.
- **REQ_0123** (framework-fault fail-fast): record the closed discrepancy — before the §3d
  precursor, an observer panic in the **cyclic** telemetry fold escaped the framework-fault
  boundary (the fold ran outside `guard_or_fatal`). Now both event and cyclic folds are inside
  it. This belongs in the discrepancy/decision record, not just the commit body.

No new requirement, no new ADR; the "one barrier + one fold per wake" itself is an internal
optimization under REQ_0060 / REQ_0268.

---

## 7. Verification gates

1. `cargo test -p taktora-executor -p taktora-executor-tests` — pinning + regression green.
2. `cargo test` workspace-wide.
3. **Linux-gated clippy on the Pi** (per the Linux-gated-clippy blind spot): the second
   `guard_or_fatal`, the grid pass, and the `#[cfg(target_os = "linux")]` `ticked`/timerfd
   drain escape macOS clippy — run CI's exact clippy line on the Pi and check
   `gh pr checks` explicitly (do not trust `--watch` exit 0 alone).
4. **Pi A/B latency — MERGE GATE.** The entire justification for #95 is latency. On the WAGO
   rig under SCHED_FIFO, measure cyclic dispatch jitter / lateness (and ideally a wake→fold
   span) **before vs after**. Merge only if it shows a measurable improvement and no telemetry
   regression. Run **two** workloads so the A/B doesn't understate the change:
   - **Pure single-cyclic.** Even here #95 removes a barrier+fold *pair* per wake: today the
     master-timer wake pays `process_attachment`'s `barrier_and_record` on the **`IGNORE`
     path** (it barriers unconditionally, even when the fired id maps to no task) **plus** the
     grid pass's own → **two** today, **one** after. Establishes the floor of the win.
   - **Multi-listener / multi-event-task.** This is where today's per-attachment barriers
     *serialize* dispatch (task *k+1* waits on task *k*'s barrier); #95 lets them run
     concurrently. This is the larger effect — omit it and the A/B argues against its own gate.

---

## 8. Risk register

| Risk | Mitigation |
|------|------------|
| `cb_result` borrow fails to compile after the second closure capture | **Resolved:** upstream `WaitSetRunResult`/`WaitSetRunError` derive `Copy` and today's code already reads it at two sites — capture is a copy, no rebind needed. |
| Stale `pending_cycle` swallows first dispatch after a test-terminal bail re-entry | **Resolved at source** by the §3e entry sweep — token lifetime is one `dispatch_loop` invocation by construction; covers all exit paths. Guarded by §5.3. |
| REQ_0123 cyclic-fold escape buried in `perf!` commit | Split into the §3d precursor commit + REQ_0123 discrepancy note (§6); guarded by §5.4. |
| Pinning test flaky (both listeners not in one wake) | Publish both **synchronously before** `run_n(1)`; both notifier events latch before the first `wait`, so the first `wait_and_process_once` sees both ready. |
| Observer panic in the moved fold escapes the framework boundary | D3's second `guard_or_fatal` re-wraps the fold → abort in prod / `None`-bail in test. Also *fixes* the pre-existing cyclic-task gap. |
| Many event jobs in flight at a `None`-bail (test terminal) | D4 defensive `pool.barrier()` before both `break Ok(())`. Double-barrier on the normal path is a counter fast-path no-op. |
| Pi A/B shows no win | Then #95 does not earn its complexity — do not merge; revisit the latency hypothesis. |
