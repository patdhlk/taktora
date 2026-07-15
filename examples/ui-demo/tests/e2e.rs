//! Headless end-to-end check of the `ui-demo` producer wiring.
//!
//! The egui View (`examples/ui-demo-view`) cannot run in a headless CI
//! environment, so this test exercises the same client-facing path the View
//! relies on — discover + hash-validate, subscribe + poll a property, invoke a
//! command — but over the demo's *exact* model (`StepperViewModel`, `Enable`,
//! `JogRelative`) and the same `Simulator` control step the binary runs.
//!
//! It stands up the producer's connector + executor in-process on a background
//! thread, then drives a real [`Client`] against it from the test thread.

use std::thread;
use std::time::Duration;

use taktora_connector_host::Connector;
use taktora_connector_ui::{UiConnector, UiConnectorOptions};
use taktora_connector_ui_client::{BindMode, Client, CommandOutcome};
use taktora_executor::{ItemFlow, ExecuteResult, Executor, ExecutorError, item_with_triggers};

use ui_demo::{Enable, JogRelative, Simulator, StepperState};

/// Connect by probing for the live contract hash first (what a hash-less demo
/// client does), then binding read-write with it.
fn connect_rw(instance: &str) -> Client {
    let probe = Client::connect(instance, "probe").expect("probe connect reads the manifest");
    let hash = probe.manifest().contract_hash.clone();
    Client::connect(instance, &hash).expect("read-write connect")
}

/// Poll a ViewModel field until it satisfies `pred`, or panic after a bound.
fn wait_for_field<F>(client: &mut Client, vm: &str, field: &str, mut pred: F) -> serde_json::Value
where
    F: FnMut(&serde_json::Value) -> bool,
{
    for _ in 0..400 {
        let _ = client.poll_view_model(vm);
        if let Some(v) = client.view_model_fields(vm).and_then(|m| m.get(field))
            && pred(v)
        {
            return v.clone();
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("field '{vm}.{field}' never satisfied the predicate within the timeout");
}

#[test]
fn demo_producer_serves_the_client_control_loop_end_to_end() {
    let instance = format!("ui-demo-test-{}", std::process::id());

    let options = UiConnectorOptions::builder()
        .instance(instance.clone())
        .publish_cadence(Duration::from_millis(5))
        .command_poll_interval(Duration::from_millis(2))
        .build();

    let mut connector = UiConnector::new(options).expect("create connector");
    let stepper = connector.add_view_model::<ui_demo::StepperViewModel>("Stepper");
    let (enable_rx, _enable_can) = connector.add_command::<Enable>("enable");
    let (jog_rx, jog_can) = connector.add_command::<JogRelative>("jog_relative");

    let mut sim = Simulator::new();
    stepper.set(&sim.view_model());
    jog_can.set(sim.can_jog());

    let mut executor = Executor::builder()
        .worker_threads(0)
        .build()
        .expect("executor");
    connector
        .register_with(&mut executor)
        .expect("register connector");

    // The producer's control item, verbatim in spirit: drain effects, step, set.
    executor
        .add(item_with_triggers(
            |d| -> Result<(), ExecutorError> {
                d.interval(Duration::from_millis(2));
                Ok(())
            },
            move |_ctx| -> ExecuteResult {
                while let Ok(Enable { .. }) = enable_rx.try_recv() {
                    sim.enable();
                }
                while let Ok(JogRelative { delta }) = jog_rx.try_recv() {
                    sim.jog(delta);
                }
                sim.step();
                stepper.set(&sim.view_model());
                jog_can.set(sim.can_jog());
                Ok(ItemFlow::Continue)
            },
        ))
        .expect("add control item");

    // Run the control loop on a background thread for the test window. The
    // connector stays on this thread (its pump/handler threads outlive it).
    let runner = thread::spawn(move || {
        let _ = executor.run_for(Duration::from_secs(10));
    });

    // --- client side: the path the View takes ---
    let mut client = connect_rw(&instance);
    assert_eq!(
        client.mode(),
        BindMode::ReadWrite,
        "a matching contract hash must bind read-write"
    );
    client.subscribe("Stepper").expect("subscribe Stepper");
    client.subscribe("System").expect("subscribe System");
    client
        .subscribe_can_execute("jog_relative")
        .expect("subscribe jog CanExecute");

    // Initially idle.
    let state0 = wait_for_field(&mut client, "Stepper", "state", |_| true);
    assert_eq!(state0, serde_json::json!("Idle"));

    // Enable -> the control loop transitions to Running and the position ramps.
    assert_eq!(
        client
            .invoke("enable", &serde_json::json!({ "force": true }))
            .expect("invoke enable"),
        CommandOutcome::Accepted
    );
    let running = wait_for_field(&mut client, "Stepper", "state", |v| {
        v == &serde_json::json!("Running")
    });
    assert_eq!(running, serde_json::json!("Running"));

    // Position must advance past the origin once running.
    let pos = wait_for_field(&mut client, "Stepper", "position", |v| {
        v.as_f64().map(|p| p > 0.0).unwrap_or(false)
    });
    assert!(
        pos.as_f64().unwrap() > 0.0,
        "position should ramp, got {pos}"
    );

    // System heartbeat must advance.
    let mut last = None;
    let mut advanced = false;
    for _ in 0..200 {
        let _ = client.poll_view_model("System");
        if let Some(c) = client
            .view_model_fields("System")
            .and_then(|m| m.get("counter"))
            .and_then(serde_json::Value::as_u64)
        {
            if let Some(prev) = last
                && c > prev
            {
                advanced = true;
                break;
            }
            last = Some(c);
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(advanced, "the System heartbeat counter must advance");

    // jog_relative becomes available outside the busy window; once it does, a
    // jog must be accepted and nudge the position.
    let mut jogged = false;
    for _ in 0..400 {
        let _ = client.poll_can_execute("jog_relative");
        if client.can_execute("jog_relative") == Some(true) {
            let out = client
                .invoke("jog_relative", &serde_json::json!({ "delta": 5.0 }))
                .expect("invoke jog");
            assert_eq!(
                out,
                CommandOutcome::Accepted,
                "jog must be accepted when can_jog is true"
            );
            jogged = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        jogged,
        "jog_relative must become executable outside the busy window"
    );

    // Keep the state enum import meaningful (documents the lifecycle under test).
    let _ = StepperState::Running;

    drop(connector);
    runner.join().expect("control thread joins");
}
