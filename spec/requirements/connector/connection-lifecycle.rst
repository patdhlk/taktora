Connection lifecycle
====================

The observable health state of every connector and the policy by which it
retries after a disconnect. This cluster ``:satisfies:`` :need:`FEAT_0030`.

.. feat:: Connection lifecycle
   :id: FEAT_0034
   :status: implemented
   :satisfies: FEAT_0030

   The observable health state of every connector and the policy by which
   a connector retries after a stack-level disconnect. Both surfaces are
   uniform across protocols, regardless of which protocol stack owns the
   reconnect mechanism.

.. req:: ConnectorHealth state machine
   :id: REQ_0230
   :status: implemented
   :satisfies: FEAT_0034
   :links: IMPL_0010, TEST_0101

   The framework shall define ``ConnectorHealth`` as an enum with
   variants ``Up``, ``Connecting { since }``, ``Degraded { reason }``,
   and ``Down { reason, since }``. Every connector shall report current
   health via ``Connector::health()``.

.. req:: subscribe_health returns a Channel of HealthEvent
   :id: REQ_0231
   :status: implemented
   :satisfies: FEAT_0034
   :links: IMPL_0040, TEST_0308

   ``Connector::subscribe_health()`` shall return an observable handle
   over the connector's ``HealthEvent`` stream so callers can wire
   health transitions into ``ExecutableItem`` triggers. The handle
   type is connector-implementation dependent — typically a
   taktora-executor ``Channel<HealthEventWire>`` (where
   ``HealthEventWire`` is the POD wire form, preferred for
   cross-process gateways) or a thin in-process wrapper around a
   ``crossbeam_channel::Receiver<HealthEvent>`` (acceptable when the
   plugin and gateway share an address space). The choice is recorded
   in the connector's ``impl::`` directive (e.g. :need:`IMPL_0040`).

.. req:: Health subscriptions are independent broadcast streams
   :id: REQ_0847
   :status: implemented
   :satisfies: FEAT_0034
   :links: IMPL_0040, IMPL_0050, IMPL_0060, IMPL_0080, TEST_0864

   Every call to ``Connector::subscribe_health()`` (and to the
   underlying health monitor's ``subscribe()``) shall return an
   **independent** stream that observes every health transition
   emitted after the call. Events shall never be load-balanced
   between subscriptions, and a transition with zero subscribers
   shall succeed (observable via ``Connector::health()``). Cloning a
   subscription handle remains a competing-consumer tap of that one
   stream and shall be documented as such.

   **Rationale.** The previous implementation handed out clones of a
   single ``crossbeam_channel`` receiver — competing consumers
   documented as broadcast. Found live on the WAGO bench (issue #60):
   a fast-polling second subscriber silently stole every event from
   the health pump, so a real unplug/recover cycle printed no
   transitions at all while the data plane worked — the observability
   surface lying by omission.

.. req:: ReconnectPolicy trait
   :id: REQ_0232
   :status: implemented
   :satisfies: FEAT_0034
   :links: BB_0001, IMPL_0010, TEST_0100

   The framework shall define a ``ReconnectPolicy`` trait with
   ``next_delay() -> Duration`` and ``reset()`` for connectors whose
   protocol stack exposes raw connect events.

.. req:: ExponentialBackoff default policy
   :id: REQ_0233
   :status: implemented
   :satisfies: FEAT_0034
   :links: IMPL_0010, TEST_0100

   The framework shall ship an ``ExponentialBackoff`` implementation of
   ``ReconnectPolicy`` configurable with initial delay, max delay, growth
   factor, and jitter ratio.

.. req:: HealthEvent emitted on every transition
   :id: REQ_0234
   :status: implemented
   :satisfies: FEAT_0034
   :links: IMPL_0010, TEST_0101

   Every transition between ``ConnectorHealth`` variants shall emit a
   ``HealthEvent`` on the connector's health channel.

.. req:: Stack-internal-reconnect connectors emit health uniformly
   :id: REQ_0235
   :status: implemented
   :satisfies: FEAT_0034
   :links: BB_0042, TEST_0308, TEST_0309

   Connectors whose underlying protocol stack manages reconnect internally
   (e.g. tonic-managed gRPC channels) shall not be required to use
   ``ReconnectPolicy``, but shall emit ``HealthEvent`` on every observed
   transition between ``ConnectorHealth`` variants.
