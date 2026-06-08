Bus health, error frames, and reconnect
=======================================

The CAN-specific health surface. This cluster ``:satisfies:``
:need:`FEAT_0046`.

.. feat:: Bus health, error frames, and reconnect
   :id: FEAT_0049
   :status: open
   :satisfies: FEAT_0046

   The CAN-specific health surface: per-interface state aggregated
   into the connector's single externally-visible
   ``ConnectorHealth``, error-frame consumption driving transitions
   internally, and ``ReconnectPolicy``-driven socket reopen on
   bus-off. Health-event semantics inherit from :need:`FEAT_0034`.

.. req:: ConnectorHealth aggregates per-iface state via worst-of
   :id: REQ_0630
   :status: approved
   :satisfies: FEAT_0049

   The single externally-visible ``ConnectorHealth`` reported by
   ``CanConnector`` shall be the worst (least-healthy) of the
   per-interface sub-states held by the gateway: any interface
   ``Down`` shall surface as ``Degraded`` while at least one
   other interface remains ``Up``, and shall surface as ``Down``
   only when every owned interface is ``Down``. Per-interface
   reasons shall be carried in the ``HealthEvent`` payload (e.g.
   ``DegradedReason::IfaceDown { iface: "can1" }``).

.. req:: Error frames consumed internally
   :id: REQ_0631
   :status: approved
   :satisfies: FEAT_0049

   The gateway shall enable the ``CAN_ERR_FLAG`` error-frame
   reporting mode on each owned interface via
   ``setsockopt(SOL_CAN_RAW, CAN_RAW_ERR_FILTER, CAN_ERR_MASK)``,
   consume error frames inside its RX loop, and use them only to
   drive ``ConnectorHealth`` transitions. Error frames shall not
   reach any plugin-visible channel (re-affirmed by
   :need:`REQ_0643`).

.. req:: error-passive transitions to Degraded
   :id: REQ_0632
   :status: approved
   :satisfies: FEAT_0049

   When an interface reports an error-passive or error-warning
   condition via an error frame, the gateway shall transition
   that interface's sub-state to ``Degraded`` with a reason
   identifying the interface and the kernel error class
   (``DegradedReason::ErrorPassive { iface }``).

.. req:: bus-off transitions to Down and triggers reconnect
   :id: REQ_0633
   :status: approved
   :satisfies: FEAT_0049

   When an interface reports a bus-off condition via an error
   frame, the gateway shall transition that interface's sub-state
   to ``Down``, close the underlying socket, and schedule a
   reopen attempt governed by the connector's
   ``ReconnectPolicy``. Once the socket is reopened, the
   gateway shall re-apply the per-interface filter
   (:need:`REQ_0622`) before transitioning back through
   ``Connecting``.

.. req:: ReconnectPolicy reused; ExponentialBackoff default
   :id: REQ_0634
   :status: approved
   :satisfies: FEAT_0049

   The CAN connector shall use the framework-level
   ``ReconnectPolicy`` trait (:need:`REQ_0232`) with
   ``ExponentialBackoff`` (:need:`REQ_0233`) as the default
   implementation, configurable via
   ``CanConnectorOptions::reconnect_policy``. This is the
   EtherCAT posture (contrast :need:`REQ_0441` for Zenoh's
   stack-internal posture) — SocketCAN exposes raw bus-off
   events and the gateway owns the reopen.

.. req:: HealthEvent emitted on every transition
   :id: REQ_0635
   :status: approved
   :satisfies: FEAT_0049

   Every transition between ``ConnectorHealth`` variants —
   including per-interface sub-state transitions that change the
   aggregated state per :need:`REQ_0630` — shall emit one
   ``HealthEvent`` on the connector's health channel
   (re-affirms :need:`REQ_0234`).

.. req:: Error frames not exposed to plugin
   :id: REQ_0636
   :status: approved
   :satisfies: FEAT_0049

   No ``ChannelReader<T>`` shall ever observe a CAN error frame
   as a ``Received<T>`` value. Error-frame visibility is confined
   to the gateway and surfaced exclusively through
   ``ConnectorHealth`` and ``HealthEvent``. This is the project
   posture chosen during brainstorming over a plugin-visible
   error channel; reconsider only if a downstream consumer
   demonstrates a concrete need.
