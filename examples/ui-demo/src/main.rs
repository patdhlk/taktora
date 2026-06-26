//! `ui-demo` producer binary — the FEAT_0092 v1 validation slice, hardware-free.
//!
//! Wires the unit-tested [`Simulator`](ui_demo::Simulator) onto a real
//! [`Executor`] + a [`UiConnector`], publishing a `Stepper`
//! [`StepperViewModel`](ui_demo::StepperViewModel) every control cycle and
//! accepting the idempotent `enable` + non-idempotent `jog_relative` commands.
//! The mandatory `System` heartbeat + the per-command `CanExecute` properties
//! are published automatically by the connector.
//!
//! Run it (`cargo run` in this directory) and point the egui View
//! (`examples/ui-demo-view`) — or `py/smoke.py` — at the `ui-demo` instance.
//! No fieldbus, no hardware, no codegen build step.

use std::time::Duration;

use taktora_executor::{ControlFlow, ExecuteResult, Executor, ExecutorError, item_with_triggers};

use taktora_connector_host::Connector;
use taktora_connector_ui::{UiConnector, UiConnectorOptions};

use ui_demo::{Enable, JogRelative, Simulator};

/// Control-loop period. 20 Hz: brisk enough to see the position ramp move and
/// `can_jog` toggle, slow enough to read the console heartbeat.
const CONTROL_PERIOD: Duration = Duration::from_millis(50);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A stable instance name so the View / smoke consumer can discover us by a
    // known namespace rather than the process name.
    let options = UiConnectorOptions::builder()
        .instance("ui-demo")
        .publish_cadence(Duration::from_millis(33)) // ~30 Hz UI updates
        .command_poll_interval(Duration::from_millis(5))
        .build();

    let mut connector = UiConnector::new(options)?;

    // Authoring phase (all before `register_with`):
    //  - one ViewModel property (the move-only writer comes back to us),
    //  - the idempotent `enable` command,
    //  - the non-idempotent `jog_relative` command.
    let stepper = connector.add_view_model::<ui_demo::StepperViewModel>("Stepper");
    let (enable_rx, _enable_can) = connector.add_command::<Enable>("enable");
    let (jog_rx, jog_can) = connector.add_command::<JogRelative>("jog_relative");

    // Seed the published value before the executor starts so a UI that connects
    // immediately sees a valid initial state (history-depth-1 redelivery).
    let mut sim = Simulator::new();
    stepper.set(&sim.view_model());
    jog_can.set(sim.can_jog());

    let mut executor = Executor::builder().worker_threads(1).build()?;
    connector.register_with(&mut executor)?;

    println!(
        "ui-demo producer up on instance 'ui-demo' — publishing Stepper + System heartbeat.\n\
         Connect the View:  (cd ../ui-demo-view && cargo run)\n\
         Or the smoke test: python3 py/smoke.py\n\
         Ctrl-C to stop."
    );

    // The single control item: drain accepted command effects, advance the
    // simulation, then republish the ViewModel + refresh the jog gate. All off
    // the connector's pump/handler threads — this runs on the executor's loop.
    let mut heartbeat: u64 = 0;
    executor.add(item_with_triggers(
        |d| -> Result<(), ExecutorError> {
            d.interval(CONTROL_PERIOD);
            Ok(())
        },
        move |_ctx| -> ExecuteResult {
            // Drain idempotent `enable` effects (re-enable is a safe no-op).
            while let Ok(Enable { force }) = enable_rx.try_recv() {
                let _ = force; // advisory in the demo
                sim.enable();
            }
            // Drain non-idempotent `jog_relative` effects; the connector already
            // gated these against the `CanExecute` we publish below.
            while let Ok(JogRelative { delta }) = jog_rx.try_recv() {
                sim.jog(delta);
            }

            // Advance one simulated control tick.
            sim.step();

            // Republish: latest-value ViewModel + the live jog gate. Setting the
            // property is alloc-free (an integer-lowered image into a seqlock).
            stepper.set(&sim.view_model());
            jog_can.set(sim.can_jog());

            // Console heartbeat ~ once per second so a backgrounded run is
            // visibly alive without flooding the terminal.
            heartbeat = heartbeat.wrapping_add(1);
            if heartbeat.is_multiple_of(20) {
                println!(
                    "tick {heartbeat:>5}  state={:?}  pos={:6.2}  can_jog={}",
                    sim.state(),
                    sim.position(),
                    sim.can_jog(),
                );
            }

            Ok(ControlFlow::Continue)
        },
    ))?;

    executor.run()?;
    Ok(())
}
