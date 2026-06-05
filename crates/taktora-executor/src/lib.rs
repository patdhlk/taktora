//! # taktora-executor
//!
//! Execution framework on top of [iceoryx2](https://docs.rs/iceoryx2).
//! Provides:
//!
//! * [`ExecutableItem`] — the unit of work the executor schedules.
//! * [`Executor`] / [`Runner`] — the run loop and a dedicated-thread host.
//! * [`Channel<T>`](Channel) / [`Service`] — pub/sub and request-response primitives
//!   with paired event services so subscribers wake on send.
//! * Sequential [chains](Executor::add_chain) and parallel
//!   [graphs](Executor::add_graph).
//! * Signal/slot via [`signal_slot::pair`].
//! * Lifecycle hooks via [`Observer`] and timing hooks via
//!   [`ExecutionMonitor`].
//!
//! See the workspace `README.md` for a quick-start.
#![doc(html_root_url = "https://docs.rs/taktora-executor/0.1.0")]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod chain;
mod channel;
mod clock;
mod condition;
mod context;
mod control_flow;
mod error;
mod executor;
mod fatal;
mod fault;
mod graph;
mod grid;
mod item;
mod monitor;
mod observer;
mod payload;
mod pool;
mod ready_ring;
mod runner;
mod service;
pub mod signal_slot;
mod stats;
mod task_id;
mod task_kind;
mod thread_attrs;
/// Linux `timerfd`-backed absolute-grid cyclic wake source (`REQ_0268`).
#[cfg(target_os = "linux")]
mod timerfd;
mod trigger;

pub use channel::{Channel, EVENT_SUFFIX, NotifyOutcome, Publisher, Subscriber};
pub use clock::{MockClock, MonotonicClock, SystemClock};
pub use condition::{Conditional, wrap_with_condition};
pub use context::{Context, Stoppable};
pub use control_flow::{ControlFlow, ExecuteResult};
pub use error::{ExecutorError, ItemError};
pub use executor::{Executor, ExecutorBuilder, ExecutorGraphBuilder};
pub use fatal::{FatalContext, FatalHandler, FatalSite};
pub use fault::{ExecutorFaultReason, ExecutorFaultState, FaultReason, FaultState};
pub use graph::{GraphBuilder, Vertex};
pub use grid::{CyclicClock, DispatchMode, MonotonicCyclicClock};
pub use item::{ExecutableItem, FnItem, FnItemWithTriggers, item, item_with_triggers};
pub use monitor::ExecutionMonitor;
pub use observer::{Observer, UserEvent};
pub use payload::Payload;
pub use runner::{Runner, RunnerFlags};
pub use service::{
    ActiveRequest, Client, PendingRequest, REQ_EVENT_SUFFIX, RESP_EVENT_SUFFIX, Server, Service,
};
pub use stats::{CycleObservation, StatsSnapshot, TaskStatsEntry};
pub use task_id::TaskId;
pub use thread_attrs::ThreadAttributes;
pub use trigger::{RawListener, TriggerDeclarer};
