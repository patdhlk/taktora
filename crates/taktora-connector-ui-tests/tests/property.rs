//! Integration tests for the server-side `Property<V>` RT-update handle.

use serde::Serialize;
use taktora_connector_ui::{ImageEnum, Property, ViewModel};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ImageEnum)]
#[repr(u8)]
enum StepperState {
    Idle = 0,
    Running = 1,
    Faulted = 2,
}

#[derive(Clone, Debug, PartialEq, Serialize, ViewModel)]
struct StepperVm {
    active: bool,
    position: f64,
    state: StepperState,
}

#[test]
fn snapshot_is_none_before_first_set() {
    let prop = Property::<StepperVm>::new();
    assert_eq!(prop.snapshot(), None);
}

#[test]
fn set_then_snapshot_round_trips() {
    let prop = Property::<StepperVm>::new();
    let vm = StepperVm {
        active: true,
        position: 12.5,
        state: StepperState::Running,
    };
    prop.set(&vm);
    assert_eq!(prop.snapshot(), Some(vm));
}

#[test]
fn snapshot_returns_latest_set() {
    let prop = Property::<StepperVm>::new();
    prop.set(&StepperVm {
        active: false,
        position: 1.0,
        state: StepperState::Idle,
    });
    let latest = StepperVm {
        active: true,
        position: 2.0,
        state: StepperState::Faulted,
    };
    prop.set(&latest);
    assert_eq!(prop.snapshot(), Some(latest));
}

#[test]
fn clone_shares_the_cell_with_the_pump_side() {
    let rt = Property::<StepperVm>::new();
    let pump = rt.clone();
    let vm = StepperVm {
        active: true,
        position: 7.0,
        state: StepperState::Running,
    };
    rt.set(&vm);
    assert_eq!(pump.snapshot(), Some(vm));
}

#[test]
fn snapshot_into_reuses_buffer_and_avoids_realloc_when_warm() {
    let prop = Property::<StepperVm>::new();
    let mut buf = Vec::new();
    assert_eq!(prop.snapshot_into(&mut buf), None);
    prop.set(&StepperVm {
        active: true,
        position: 3.0,
        state: StepperState::Idle,
    });
    let got = prop.snapshot_into(&mut buf).unwrap();
    assert_eq!(got.position, 3.0);
    let cap = buf.capacity();
    // Second warm call must not grow the buffer.
    prop.set(&StepperVm {
        active: false,
        position: 4.0,
        state: StepperState::Running,
    });
    let _ = prop.snapshot_into(&mut buf).unwrap();
    assert_eq!(buf.capacity(), cap, "warm snapshot reallocated");
}

#[test]
fn concurrent_set_and_snapshot_never_reconstructs_an_invalid_image() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    let rt = Arc::new(Property::<StepperVm>::new());
    let stop = Arc::new(AtomicBool::new(false));

    let writer = {
        let rt = Arc::clone(&rt);
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            let states = [
                StepperState::Idle,
                StepperState::Running,
                StepperState::Faulted,
            ];
            let mut i = 0usize;
            while !stop.load(Ordering::Relaxed) {
                let s = states[i % states.len()];
                // Tie position to the state so a torn read would surface as an
                // (position, state) pair the writer never published.
                let position = match s {
                    StepperState::Idle => 1.0,
                    StepperState::Running => 2.0,
                    StepperState::Faulted => 3.0,
                };
                rt.set(&StepperVm {
                    active: true,
                    position,
                    state: s,
                });
                i += 1;
            }
        })
    };

    let reader = rt.clone();
    let mut buf = Vec::new();
    for _ in 0..200_000 {
        if let Some(vm) = reader.snapshot_into(&mut buf) {
            let expected = match vm.state {
                StepperState::Idle => 1.0,
                StepperState::Running => 2.0,
                StepperState::Faulted => 3.0,
            };
            assert_eq!(
                vm.position, expected,
                "torn read: state {:?} with position {}",
                vm.state, vm.position
            );
        }
    }

    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();
}
