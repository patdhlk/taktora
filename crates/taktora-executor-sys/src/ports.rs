//! `Send` wrappers for iceoryx2 `ipc::Service` ports.

use core::mem::MaybeUninit;
use iceoryx2::port::client::{Client as IxClient, RequestSendError};
use iceoryx2::port::notifier::{Notifier as IxNotifier, NotifierNotifyError};
use iceoryx2::port::publisher::Publisher as IxPublisher;
use iceoryx2::port::server::Server as IxServer;
use iceoryx2::port::subscriber::Subscriber as IxSubscriber;
use iceoryx2::port::{LoanError, ReceiveError, SendError};
use iceoryx2::prelude::*;
use iceoryx2::response::Response as IxResponse;
use iceoryx2::sample::Sample as IxSample;
use iceoryx2::sample_mut::SampleMut;
use iceoryx2::sample_mut_uninit::SampleMutUninit;
use std::sync::Arc;

type IpcService = ipc::Service;

/// `Send` wrapper for iceoryx2 `Publisher<ipc::Service, T, ()>`.
///
/// # Soundness
///
/// Exposes only methods that are safe to call after port creation. The wrapped
/// `IxPublisher` is `!Send` only because the `ipc::Service` arc policy is
/// `SingleThreaded`, holding an `Rc` that is mutated during port creation.
/// After construction, only `send_copy` and `loan_uninit` are exposed; neither
/// touches the Rc concurrently.
pub struct SendPublisher<T: core::fmt::Debug + ZeroCopySend + 'static> {
    inner: IxPublisher<IpcService, T, ()>,
}

#[allow(unsafe_code)]
#[allow(clippy::non_send_fields_in_send_ty)]
// SAFETY: `IxPublisher<ipc::Service, T, ()>` is `!Send` only because of the
// `SingleThreaded` Rc; after port creation, `send_copy(...)` and `loan_uninit(...)`
// don't touch the Rc concurrently. Move-only, no Sync.
unsafe impl<T: core::fmt::Debug + ZeroCopySend + 'static> Send for SendPublisher<T> {}

impl<T: core::fmt::Debug + ZeroCopySend + 'static> SendPublisher<T> {
    /// Wrap an iceoryx2 publisher, asserting that it will only be used via
    /// the safe methods exposed by this wrapper.
    #[must_use]
    pub const fn new(inner: IxPublisher<IpcService, T, ()>) -> Self {
        Self { inner }
    }

    /// Send by value (copies). Forwards to `IxPublisher::send_copy`.
    pub fn send_copy(&self, value: T) -> Result<usize, SendError>
    where
        T: Copy,
    {
        self.inner.send_copy(value)
    }

    /// Loan an uninitialised sample. Forwards to `IxPublisher::loan_uninit`.
    pub fn loan_uninit(
        &self,
    ) -> Result<SampleMutUninit<IpcService, MaybeUninit<T>, ()>, LoanError> {
        self.inner.loan_uninit()
    }

    /// True zero-copy loan: the closure receives `&mut MaybeUninit<T>` and
    /// must fully initialize it before returning `true`. Returning `false`
    /// skips initialization and returns `Ok(None)`.
    ///
    /// # Contract
    ///
    /// **Returning `true` from the closure asserts that the payload is
    /// fully initialized.** Returning `true` without writing a valid `T`
    /// causes undefined behaviour at the subsequent `assume_init` step.
    ///
    /// # Safety
    ///
    /// This method contains `unsafe` code: after the closure returns `true`,
    /// it calls `sample.assume_init()` asserting that the payload was
    /// initialized. This is sound only if the closure upholds its contract.
    #[allow(unsafe_code)]
    pub fn loan_init<F>(&self, f: F) -> Result<Option<SampleMut<IpcService, T, ()>>, LoanError>
    where
        F: FnOnce(&mut MaybeUninit<T>) -> bool,
    {
        let mut sample = self.inner.loan_uninit()?;
        let cont = f(sample.payload_mut());
        if !cont {
            // Closure declined to initialize; return None.
            return Ok(None);
        }
        // SAFETY: the closure returned `true`, asserting that the payload was
        // fully initialised before this point. Per the documented contract,
        // a closure that returns `true` without writing a valid `T` is a
        // contract violation and the resulting behaviour is undefined.
        let sample = unsafe { sample.assume_init() };
        Ok(Some(sample))
    }
}

/// `Send` wrapper for iceoryx2 `Subscriber<ipc::Service, T, ()>`.
///
/// # Soundness
///
/// Exposes only methods safe to call after port creation. The wrapped
/// `IxSubscriber` is `!Send` because `ipc::Service::ArcThreadSafetyPolicy` is
/// `SingleThreaded`, holding an `Rc` mutated only during port creation. After
/// construction, only `receive()` is exposed, which performs a pure shared-memory
/// read without touching the Rc.
pub struct SendSubscriber<T: core::fmt::Debug + ZeroCopySend + 'static> {
    inner: IxSubscriber<IpcService, T, ()>,
}

// SAFETY:
// `IxSubscriber<ipc::Service, T, ()>` is `!Send` because the `ipc::Service`
// `ArcThreadSafetyPolicy` is `SingleThreaded`, which holds an `Rc<...>`.
// The Rc is mutated only when methods that call `lock()` on the policy
// run — primarily during port creation. After construction, only
// `receive()` is called (does not touch the Rc; pure shared-memory read path).
// No two threads concurrently mutate the same Rc refcount, so moving a
// `SendSubscriber` to a pool worker is sound. We do not implement `Sync`;
// `SendSubscriber` is move-only across threads, never shared.
#[allow(unsafe_code)]
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T: core::fmt::Debug + ZeroCopySend + 'static> Send for SendSubscriber<T> {}

impl<T: core::fmt::Debug + ZeroCopySend + 'static> SendSubscriber<T> {
    /// Wrap an iceoryx2 subscriber, asserting that it will only be used via
    /// the safe methods exposed by this wrapper.
    #[must_use]
    pub const fn new(inner: IxSubscriber<IpcService, T, ()>) -> Self {
        Self { inner }
    }

    /// Receive the next sample, if any. Forwards to `IxSubscriber::receive`.
    pub fn receive(&self) -> Result<Option<IxSample<IpcService, T, ()>>, ReceiveError> {
        self.inner.receive()
    }
}

/// `Send` wrapper for iceoryx2 `Notifier<ipc::Service>`.
///
/// # Soundness
///
/// Exposes only `notify()`, which writes into a lock-free shared-memory ring
/// without touching the `SingleThreaded` Rc arc policy created during port
/// construction.
pub struct SendNotifier {
    inner: IxNotifier<IpcService>,
}

// SAFETY: `IxNotifier<ipc::Service>` is `!Send` only because `ipc::Service`
// uses `SingleThreaded` (an `Rc`-backed arc policy) which is mutated only at
// port-construction time. After the notifier is created, the only operation
// we perform on it from any thread is `notifier.notify()`, which does not
// touch the `Rc` refcount — it writes into a lock-free shared memory ring.
// We never expose a `&mut Notifier` across thread boundaries and we do not
// implement `Sync`, so concurrent mutation of the Rc is impossible. Moving
// the notifier across threads is therefore sound.
#[allow(unsafe_code)]
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for SendNotifier {}

impl SendNotifier {
    /// Wrap an iceoryx2 notifier, asserting that it will only be used via
    /// the safe methods exposed by this wrapper.
    #[must_use]
    pub const fn new(inner: IxNotifier<IpcService>) -> Self {
        Self { inner }
    }

    /// Notify all listeners. Forwards to `IxNotifier::notify`.
    pub fn notify(&self) -> Result<usize, NotifierNotifyError> {
        self.inner.notify()
    }
}

/// Shared `Send + Sync` wrapper for iceoryx2 `Notifier<ipc::Service>`.
///
/// # Soundness
///
/// Wraps `Arc<IxNotifier<ipc::Service>>` and exposes only `notify()`. The wrapped
/// notifier is `!Send + !Sync` because `ipc::Service` uses a `SingleThreaded`
/// (Rc-backed) arc policy mutated only at port-construction time. After the
/// notifier is created and wrapped in `Arc`, the only operation performed from
/// any thread is `notify()`, which writes into a lock-free shared-memory ring
/// without touching the `Rc` refcount. `Arc::clone` and `Arc::drop` touch only
/// the `Arc`'s own refcount, not the iceoryx2 `Rc`. No `&mut Notifier` is exposed
/// across thread boundaries, so concurrent mutation of the iceoryx2 `Rc` is
/// impossible. Sharing the `Arc<Notifier>` across threads is therefore sound.
#[derive(Clone)]
pub struct SharedNotifier(Arc<IxNotifier<IpcService>>);

// SAFETY: `IxNotifier<ipc::Service>` is `!Send` only because `ipc::Service`
// uses `SingleThreaded` (an `Rc`-backed arc policy) which is mutated only at
// port-construction time. After the notifier is created and wrapped in `Arc`,
// the only operation we perform on it from any thread is `notifier.notify()`,
// which does not touch the `Rc` refcount — it writes into a lock-free shared
// memory ring. We never expose a `&mut Notifier` across thread boundaries and
// `Arc::clone`/`Arc::drop` touch only the Arc's refcount, not the iceoryx2 Rc,
// so concurrent mutation of the Rc is impossible. Moving and sharing the
// Arc<Notifier> across threads is therefore sound.
#[allow(unsafe_code)]
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for SharedNotifier {}

// SAFETY: Same rationale as Send. `notify()` is the only exposed operation,
// which writes to a lock-free shared-memory ring without touching the
// SingleThreaded Rc. Arc clone/drop operations are thread-safe by construction.
#[allow(unsafe_code)]
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Sync for SharedNotifier {}

impl SharedNotifier {
    /// Wrap an iceoryx2 notifier in an `Arc`, asserting that it will only be
    /// used via the safe methods exposed by this wrapper.
    #[must_use]
    pub const fn new(inner: Arc<IxNotifier<IpcService>>) -> Self {
        Self(inner)
    }

    /// Notify all listeners. Forwards to `IxNotifier::notify`.
    pub fn notify(&self) -> Result<usize, NotifierNotifyError> {
        self.0.notify()
    }
}

/// `Send` wrapper for iceoryx2 `Server<ipc::Service, Req, (), Resp, ()>`.
///
/// # Soundness
///
/// Exposes only methods safe to call after port creation. The wrapped
/// `IxServer` is `!Send` because `ipc::Service::ArcThreadSafetyPolicy` is
/// `SingleThreaded`, wrapping an Rc-like interior. The Rc is only mutated
/// during port creation (constructor) and during `update_connections` (called
/// inside `receive()`). After construction, only `receive()` is exposed —
/// which drives `update_connections()` plus a shared-memory read.
pub struct SendServer<Req, Resp>
where
    Req: core::fmt::Debug + ZeroCopySend + 'static,
    Resp: core::fmt::Debug + ZeroCopySend + 'static,
{
    inner: IxServer<IpcService, Req, (), Resp, ()>,
}

// SAFETY: `IxServer<ipc::Service, …>` is `!Send` because
// `ipc::Service::ArcThreadSafetyPolicy` is `SingleThreaded`, which wraps an
// `Rc`-like interior. The Rc is only mutated during port creation (constructor)
// and during `update_connections` (called inside `receive()`). After
// construction, only `receive()` is called — which drives `update_connections()`
// plus a shared-memory read. No two threads concurrently touch the Rc, so
// moving a `SendServer` is sound. We do not implement `Sync`; the struct is
// move-only across threads.
#[allow(unsafe_code)]
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<Req, Resp> Send for SendServer<Req, Resp>
where
    Req: core::fmt::Debug + ZeroCopySend + 'static,
    Resp: core::fmt::Debug + ZeroCopySend + 'static,
{
}

impl<Req, Resp> SendServer<Req, Resp>
where
    Req: core::fmt::Debug + ZeroCopySend + 'static,
    Resp: core::fmt::Debug + ZeroCopySend + 'static,
{
    /// Wrap an iceoryx2 server, asserting that it will only be used via
    /// the safe methods exposed by this wrapper.
    #[must_use]
    pub const fn new(inner: IxServer<IpcService, Req, (), Resp, ()>) -> Self {
        Self { inner }
    }

    /// Take the next pending request, if any. Forwards to `IxServer::receive`.
    #[allow(clippy::type_complexity)]
    pub fn receive(
        &self,
    ) -> Result<
        Option<iceoryx2::active_request::ActiveRequest<IpcService, Req, (), Resp, ()>>,
        ReceiveError,
    > {
        self.inner.receive()
    }
}

/// `Send` wrapper for iceoryx2 `Client<ipc::Service, Req, (), Resp, ()>`.
///
/// # Soundness
///
/// Exposes only methods safe to call after port creation. The wrapped
/// `IxClient` is `!Send` because `SingleThreaded` holds an Rc. After
/// construction, only `send_copy` is exposed, which does not touch the Rc
/// concurrently.
pub struct SendClient<Req, Resp>
where
    Req: core::fmt::Debug + ZeroCopySend + 'static,
    Resp: core::fmt::Debug + ZeroCopySend + 'static,
{
    inner: IxClient<IpcService, Req, (), Resp, ()>,
}

// SAFETY: `IxClient<ipc::Service, …>` is `!Send` because `SingleThreaded`
// holds an Rc. After construction, only `send_copy` is called. `send_copy`
// does not touch the Rc concurrently. No concurrent Rc mutation, so moving a
// `SendClient` is sound. We do not implement `Sync`.
#[allow(unsafe_code)]
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<Req, Resp> Send for SendClient<Req, Resp>
where
    Req: core::fmt::Debug + ZeroCopySend + 'static,
    Resp: core::fmt::Debug + ZeroCopySend + 'static,
{
}

impl<Req, Resp> SendClient<Req, Resp>
where
    Req: core::fmt::Debug + ZeroCopySend + 'static,
    Resp: core::fmt::Debug + ZeroCopySend + 'static,
{
    /// Wrap an iceoryx2 client, asserting that it will only be used via
    /// the safe methods exposed by this wrapper.
    #[must_use]
    pub const fn new(inner: IxClient<IpcService, Req, (), Resp, ()>) -> Self {
        Self { inner }
    }

    /// Send a request by value (copies). Forwards to `IxClient::send_copy`.
    pub fn send_copy(
        &self,
        req: Req,
    ) -> Result<
        iceoryx2::pending_response::PendingResponse<IpcService, Req, (), Resp, ()>,
        RequestSendError,
    >
    where
        Req: Copy,
    {
        self.inner.send_copy(req)
    }
}

/// `Send` wrapper for iceoryx2 `PendingResponse<ipc::Service, Req, (), Resp, ()>`.
///
/// # Soundness
///
/// Exposes only `receive()`, which performs a shared-memory read path without
/// touching the `SingleThreaded` Rc. After construction, no concurrent Rc
/// mutation occurs.
pub struct SendPendingRequest<Req, Resp>
where
    Req: core::fmt::Debug + ZeroCopySend + 'static,
    Resp: core::fmt::Debug + ZeroCopySend + 'static,
{
    inner: iceoryx2::pending_response::PendingResponse<IpcService, Req, (), Resp, ()>,
}

// SAFETY: `PendingResponse<ipc::Service, …>` is `!Send` for the same
// `SingleThreaded` Rc reason. After construction, only `receive()` is
// called (shared-memory read path, no concurrent Rc mutation).
// Move-only across threads; no `Sync`.
#[allow(unsafe_code)]
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<Req, Resp> Send for SendPendingRequest<Req, Resp>
where
    Req: core::fmt::Debug + ZeroCopySend + 'static,
    Resp: core::fmt::Debug + ZeroCopySend + 'static,
{
}

impl<Req, Resp> SendPendingRequest<Req, Resp>
where
    Req: core::fmt::Debug + ZeroCopySend + 'static,
    Resp: core::fmt::Debug + ZeroCopySend + 'static,
{
    /// Wrap an iceoryx2 pending response, asserting that it will only be used
    /// via the safe methods exposed by this wrapper.
    #[must_use]
    pub const fn new(
        inner: iceoryx2::pending_response::PendingResponse<IpcService, Req, (), Resp, ()>,
    ) -> Self {
        Self { inner }
    }

    /// Try to receive the next response, if one has arrived.
    /// Forwards to `PendingResponse::receive`.
    pub fn receive(&self) -> Result<Option<IxResponse<IpcService, Resp, ()>>, ReceiveError> {
        self.inner.receive()
    }
}

/// `Send` wrapper for `Arc<Listener<ipc::Service>>`.
///
/// # Soundness
///
/// `Listener` (iceoryx2's `IxListener`) is `!Send` because `ipc::Service`
/// uses a `SingleThreaded` Rc-backed arc policy mutated only during port
/// creation. After the listener is created and wrapped in `Arc`, the only
/// operation performed is `try_wait_one()`, which does not touch the `Rc`
/// refcount. `Arc::clone` and `Arc::drop` touch only the `Arc`'s refcount,
/// not the iceoryx2 `Rc`. No `&mut Listener` is exposed across thread
/// boundaries, so concurrent mutation of the iceoryx2 `Rc` is impossible.
/// Moving the `Arc<Listener>` across threads is therefore sound.
///
/// Used for `Executor::stop_listener` so `Executor` is auto-`Send` (it is
/// moved into Runner threads).
#[derive(Clone, Debug)]
pub struct SendListener(Arc<iceoryx2::port::listener::Listener<ipc::Service>>);

// SAFETY: `IxListener<ipc::Service>` is `!Send` because `ipc::Service` uses
// `SingleThreaded` (Rc-backed arc policy) mutated only at port-creation time.
// After the listener is created and wrapped in `Arc`, the only operation we
// perform is `try_wait_one()`, which does not touch the `Rc` refcount. We
// never expose `&mut Listener` across thread boundaries, and `Arc::clone` /
// `Arc::drop` touch only the Arc's refcount, not the iceoryx2 Rc. Moving the
// `Arc<Listener>` is therefore sound. Not `Sync` — move-only.
#[allow(unsafe_code)]
#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for SendListener {}

impl SendListener {
    /// Wrap an `Arc<Listener>` for cross-thread transfer.
    #[must_use]
    pub const fn new(listener: Arc<iceoryx2::port::listener::Listener<ipc::Service>>) -> Self {
        Self(listener)
    }

    /// Access the inner listener.
    #[must_use]
    pub fn get(&self) -> &iceoryx2::port::listener::Listener<ipc::Service> {
        &self.0
    }

    /// Clone the inner `Arc<Listener>`.
    #[must_use]
    pub fn clone_inner(&self) -> Arc<iceoryx2::port::listener::Listener<ipc::Service>> {
        Arc::clone(&self.0)
    }
}
