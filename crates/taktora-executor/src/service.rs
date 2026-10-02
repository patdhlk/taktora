//! `Service<Req, Resp>` — iceoryx2 request/response paired with two event
//! services (one each for request- and response-available wakeups).

use crate::error::ExecutorError;
use crate::payload::Payload;
use core::marker::PhantomData;
use iceoryx2::node::Node;
use iceoryx2::port::listener::Listener as IxListener;
use iceoryx2::prelude::*;
use iceoryx2::response::Response as IxResponse;
use std::sync::Arc;
use taktora_executor_sys::ports::{
    SendClient, SendListener, SendNotifier, SendPendingRequest, SendServer,
};

type IpcService = ipc::Service;

/// Suffix appended to a service name to form the request-available event service name.
pub const REQ_EVENT_SUFFIX: &str = ".__taktora_req_event";

/// Suffix appended to a service name to form the response-available event service name.
pub const RESP_EVENT_SUFFIX: &str = ".__taktora_resp_event";

/// Request/response service with two paired event services for wakeup.
pub struct Service<Req, Resp>
where
    Req: Payload,
    Resp: Payload,
{
    rr: iceoryx2::service::port_factory::request_response::PortFactory<
        IpcService,
        Req,
        (),
        Resp,
        (),
    >,
    req_event: iceoryx2::service::port_factory::event::PortFactory<IpcService>,
    resp_event: iceoryx2::service::port_factory::event::PortFactory<IpcService>,
    _marker: PhantomData<(Req, Resp)>,
}

impl<Req, Resp> Service<Req, Resp>
where
    Req: Payload,
    Resp: Payload,
{
    /// Open or create the service by name, creating the two paired event services.
    pub fn open_or_create(
        node: &Node<IpcService>,
        topic: &str,
    ) -> Result<Arc<Self>, ExecutorError> {
        let rr_name = topic
            .try_into()
            .map_err(|e| ExecutorError::Builder(format!("invalid service name: {e:?}")))?;
        let rr = node
            .service_builder(&rr_name)
            .request_response::<Req, Resp>()
            .open_or_create()
            .map_err(ExecutorError::iceoryx2)?;

        let make_event = |suffix: &str| -> Result<_, ExecutorError> {
            let n = format!("{topic}{suffix}");
            let n = n
                .as_str()
                .try_into()
                .map_err(|e| ExecutorError::Builder(format!("invalid event-topic name: {e:?}")))?;
            node.service_builder(&n)
                .event()
                .open_or_create()
                .map_err(ExecutorError::iceoryx2)
        };
        let req_event = make_event(REQ_EVENT_SUFFIX)?;
        let resp_event = make_event(RESP_EVENT_SUFFIX)?;

        Ok(Arc::new(Self {
            rr,
            req_event,
            resp_event,
            _marker: PhantomData,
        }))
    }

    /// Create a new `Server` that listens for requests on this service.
    pub fn server(self: &Arc<Self>) -> Result<Server<Req, Resp>, ExecutorError> {
        let inner = self
            .rr
            .server_builder()
            .create()
            .map_err(ExecutorError::iceoryx2)?;
        let listener = self
            .req_event
            .listener_builder()
            .create()
            .map_err(ExecutorError::iceoryx2)?;
        let resp_notifier = self
            .resp_event
            .notifier_builder()
            .create()
            .map_err(ExecutorError::iceoryx2)?;
        // SAFETY: After port creation, only `try_wait_one()` is called on the
        // listener, which does not touch the iceoryx2 `Rc` refcount. Moving the
        // Arc<Listener> across threads is sound per SendListener's contract.
        #[allow(clippy::arc_with_non_send_sync)]
        let listener = SendListener::new(Arc::new(listener));
        Ok(Server {
            inner: SendServer::new(inner),
            listener,
            resp_notifier: SendNotifier::new(resp_notifier),
            _service: Arc::clone(self),
        })
    }

    /// Create a new `Client` that sends requests on this service.
    pub fn client(self: &Arc<Self>) -> Result<Client<Req, Resp>, ExecutorError> {
        let inner = self
            .rr
            .client_builder()
            .create()
            .map_err(ExecutorError::iceoryx2)?;
        let listener = self
            .resp_event
            .listener_builder()
            .create()
            .map_err(ExecutorError::iceoryx2)?;
        let req_notifier = self
            .req_event
            .notifier_builder()
            .create()
            .map_err(ExecutorError::iceoryx2)?;
        // SAFETY: After port creation, only `try_wait_one()` is called on the
        // listener, which does not touch the iceoryx2 `Rc` refcount. Moving the
        // Arc<Listener> across threads is sound per SendListener's contract.
        #[allow(clippy::arc_with_non_send_sync)]
        let listener = SendListener::new(Arc::new(listener));
        Ok(Client {
            inner: SendClient::new(inner),
            listener,
            req_notifier: SendNotifier::new(req_notifier),
            _service: Arc::clone(self),
        })
    }
}

/// Server side of a `Service<Req, Resp>`. Receives requests and sends responses.
pub struct Server<Req, Resp>
where
    Req: Payload,
    Resp: Payload,
{
    inner: SendServer<Req, Resp>,
    listener: SendListener,
    resp_notifier: SendNotifier,
    _service: Arc<Service<Req, Resp>>,
}

impl<Req, Resp> Server<Req, Resp>
where
    Req: Payload + Copy,
    Resp: Payload + Copy,
{
    /// Take the next pending request, if any.
    ///
    /// Returns `(payload_copy, ActiveRequest)`. Use the `ActiveRequest` to
    /// respond via `respond_copy`.
    #[allow(clippy::type_complexity, clippy::option_if_let_else)]
    pub fn take_request(
        &self,
    ) -> Result<Option<(Req, ActiveRequest<'_, Req, Resp>)>, ExecutorError> {
        match self.inner.receive().map_err(ExecutorError::iceoryx2)? {
            None => Ok(None),
            Some(active) => {
                let req = *active;
                Ok(Some((
                    req,
                    ActiveRequest {
                        active,
                        server: self,
                    },
                )))
            }
        }
    }

    /// Borrow the request-event listener (executor uses this for trigger attachment).
    pub fn listener_handle(&self) -> Arc<IxListener<IpcService>> {
        self.listener.clone_inner()
    }
}

/// A received request, used to send the response.
pub struct ActiveRequest<'s, Req, Resp>
where
    Req: Payload,
    Resp: Payload,
{
    active: iceoryx2::active_request::ActiveRequest<IpcService, Req, (), Resp, ()>,
    server: &'s Server<Req, Resp>,
}

impl<Req, Resp> ActiveRequest<'_, Req, Resp>
where
    Req: Payload + Copy,
    Resp: Payload + Copy,
{
    /// Send a response by value and notify the client's listener.
    pub fn respond_copy(self, resp: Resp) -> Result<(), ExecutorError> {
        let sample = self.active.loan_uninit().map_err(ExecutorError::iceoryx2)?;
        let sample = sample.write_payload(resp);
        sample.send().map_err(ExecutorError::iceoryx2)?;
        self.server
            .resp_notifier
            .notify()
            .map_err(ExecutorError::iceoryx2)?;
        Ok(())
    }
}

/// Client side of a `Service<Req, Resp>`. Sends requests and receives responses.
pub struct Client<Req, Resp>
where
    Req: Payload,
    Resp: Payload,
{
    inner: SendClient<Req, Resp>,
    listener: SendListener,
    req_notifier: SendNotifier,
    _service: Arc<Service<Req, Resp>>,
}

impl<Req, Resp> Client<Req, Resp>
where
    Req: Payload + Copy,
    Resp: Payload + Copy,
{
    /// Send a request by value. Returns a `PendingRequest` handle for receiving
    /// the response(s), and notifies the server's listener.
    pub fn send_copy(&self, req: Req) -> Result<PendingRequest<Req, Resp>, ExecutorError> {
        let pending = self.inner.send_copy(req).map_err(ExecutorError::iceoryx2)?;
        self.req_notifier
            .notify()
            .map_err(ExecutorError::iceoryx2)?;
        Ok(PendingRequest {
            inner: SendPendingRequest::new(pending),
        })
    }

    /// Borrow the response-event listener (executor uses this for trigger attachment).
    pub fn listener_handle(&self) -> Arc<IxListener<IpcService>> {
        self.listener.clone_inner()
    }
}

/// Handle to an in-flight request — receives the matching response(s).
pub struct PendingRequest<Req, Resp>
where
    Req: Payload,
    Resp: Payload,
{
    inner: SendPendingRequest<Req, Resp>,
}

impl<Req, Resp> PendingRequest<Req, Resp>
where
    Req: Payload + Copy,
    Resp: Payload + Copy,
{
    /// Try to receive the next response, if one has arrived.
    ///
    /// The iceoryx2 0.8.1 `PendingResponse::receive()` returns a
    /// `Response<IpcService, Resp, ()>`, not a `Sample` — this wraps it.
    pub fn take(&self) -> Result<Option<IxResponse<IpcService, Resp, ()>>, ExecutorError> {
        self.inner.receive().map_err(ExecutorError::iceoryx2)
    }
}
