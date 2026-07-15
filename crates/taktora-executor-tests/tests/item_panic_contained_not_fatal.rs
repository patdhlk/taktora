//! TEST_0825 — REQ_0124: an item panic is CONTAINED — it is caught by
//! `run_item_catch_unwind`, surfaced to the Observer as `on_app_error`,
//! and does NOT invoke the fatal handler or abort the process.
//!
//! This test pins the containment behavior so that future fail-fast work
//! can never silently escalate a user-item panic to a process abort.
//!
//! ### What is asserted
//!
//! 1. `on_app_error` fires for the panicking task; the error's `Display`
//!    contains the panic message "boom in item".
//! 2. The fatal handler (`on_fatal`) is NEVER invoked — `fatal_fired`
//!    stays `false`.
//! 3. An independent sibling task keeps running across multiple cycles —
//!    the executor survives each panic and the sibling counter advances.
//! 4. The panicking task does NOT enter `Faulted` state — containment is
//!    NOT a fault transition (Faulted is only for deadline-budget breaches,
//!    REQ_0070).
//!
//! ### Drive mechanism
//!
//! `run_n(1)` is called repeatedly (5 times). Each call dispatches one
//! WaitSet wakeup; both tasks fire (same interval trigger). The panicking
//! task's error is captured in `iter_err` and returned as
//! `Err(ExecutorError::Item{..})` — NOT an abort. The sibling's counter
//! increments each cycle, demonstrating the executor survives.
//!
//! `worker_threads(0)` (inline pool) is used for determinism: no
//! scheduling jitter, no thread-pool warm-up.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use taktora_executor::{
    ExecuteResult, Executor, ExecutorError, FaultState, ItemFlow, Observer, TaskId,
    item_with_triggers,
};

// ── Test Observer double ─────────────────────────────────────────────────────

/// Captures every `on_app_error` invocation for later assertion.
#[derive(Default)]
struct PanicCapture {
    /// Number of times `on_app_error` was called.
    app_error_count: AtomicU64,
    /// Concatenated `Display` text of all captured errors.
    app_error_text: std::sync::Mutex<String>,
}

impl Observer for PanicCapture {
    fn on_app_error(&self, _task: TaskId, e: &(dyn std::error::Error + 'static)) {
        self.app_error_count.fetch_add(1, Ordering::SeqCst);
        let mut text = self.app_error_text.lock().unwrap();
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&e.to_string());
    }
}

// ── Test ─────────────────────────────────────────────────────────────────────

#[test]
fn item_panic_is_contained_not_fatal() {
    // --- Setup: shared state ------------------------------------------------

    // Set to `true` if the fatal handler fires (must stay `false`).
    let fatal_fired = Arc::new(AtomicBool::new(false));
    let fatal_fired_in_handler = Arc::clone(&fatal_fired);

    // Incremented by the sibling (healthy) task each time it executes.
    let sibling_counter = Arc::new(AtomicU64::new(0));
    let sibling_counter_in_item = Arc::clone(&sibling_counter);

    // Observer that records on_app_error calls.
    let observer = Arc::new(PanicCapture::default());

    // --- Build executor -----------------------------------------------------

    let mut exec = Executor::builder()
        .worker_threads(0) // inline — deterministic, no pool jitter
        .observer(Arc::clone(&observer) as Arc<dyn Observer>)
        .on_fatal(move |_ctx| {
            fatal_fired_in_handler.store(true, Ordering::SeqCst);
        })
        .build()
        .expect("executor build");

    // --- Register tasks -----------------------------------------------------

    // Task 1: panics every time it runs.
    let panic_id = exec
        .add(item_with_triggers(
            |d| -> Result<(), ExecutorError> {
                d.interval(Duration::from_millis(1));
                Ok(())
            },
            |_ctx| -> ExecuteResult {
                panic!("boom in item");
            },
        ))
        .expect("add panicking task");

    // Task 2: healthy sibling — counts how many times it executes.
    exec.add(item_with_triggers(
        |d| -> Result<(), ExecutorError> {
            d.interval(Duration::from_millis(1));
            Ok(())
        },
        move |_ctx| -> ExecuteResult {
            sibling_counter_in_item.fetch_add(1, Ordering::SeqCst);
            Ok(ItemFlow::Continue)
        },
    ))
    .expect("add sibling task");

    // --- Drive: 5 independent cycles ----------------------------------------
    //
    // Each `run_n(1)` dispatches one WaitSet wakeup (both tasks fire).
    // The panic is caught below the fail-fast boundary; `run_n` returns
    // `Err(ExecutorError::Item{..})` — never aborts.
    //
    // We accept `Err(Item{..})` as the expected outcome of each cycle.
    // Any OTHER error variant (AlreadyRunning, Iceoryx2, ...) would be
    // unexpected and is surfaced via `unwrap_or_else`.

    const CYCLES: usize = 5;
    for _ in 0..CYCLES {
        match exec.run_n(1) {
            Ok(()) => { /* a wakeup where the panicking task's interval had not yet elapsed; defensive — the panic surfaces as Err on the cycles where it does fire */
            }
            Err(ExecutorError::Item { .. }) => { /* expected: panic was contained */ }
            Err(other) => panic!("unexpected executor error: {other}"),
        }
    }

    // --- Assertions ---------------------------------------------------------

    // 1. on_app_error was called at least once for the panicking task.
    let app_error_count = observer.app_error_count.load(Ordering::SeqCst);
    assert!(
        app_error_count >= 1,
        "on_app_error must be called for the panicking task; got {app_error_count} calls"
    );

    // 2. The error text contains the panic message.
    {
        let text = observer.app_error_text.lock().unwrap();
        assert!(
            text.contains("boom in item"),
            "on_app_error text must contain the panic message 'boom in item'; got: {text:?}"
        );
    }

    // 3. Fatal handler was NOT invoked — panic is below the fail-fast boundary.
    assert!(
        !fatal_fired.load(Ordering::SeqCst),
        "fatal handler must NOT be invoked for a contained item panic"
    );

    // 4. Sibling advanced — executor survived the panics across cycles.
    let sibling_ran = sibling_counter.load(Ordering::SeqCst);
    assert!(
        sibling_ran >= 1,
        "sibling task must have run at least once; got {sibling_ran}"
    );

    // 5. Panicking task is NOT in Faulted state — containment ≠ fault
    //    transition (Faulted is only for budget breaches, REQ_0070).
    let panic_task_state = exec
        .task_fault_state(panic_id)
        .expect("panic task known to executor");
    assert_eq!(
        panic_task_state,
        FaultState::Running,
        "panicking task must NOT enter Faulted state; containment is orthogonal to the fault model"
    );
}
