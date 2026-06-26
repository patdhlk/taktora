Liveness, lifecycle and trust
=============================

How a UI knows the application is alive and fresh, how each side survives the
other restarting, and the trust boundary for v1. This cluster ``:satisfies:``
:need:`FEAT_0092`.

.. feat:: Liveness, lifecycle and trust
   :id: FEAT_0096
   :status: open
   :satisfies: FEAT_0092

   A mandatory ``SystemViewModel`` heartbeat carries a monotonic counter and a
   process epoch, giving the UI one canonical alive-and-fresh signal;
   per-ViewModel staleness is derivable from the envelope timestamp. UI restart
   is stateless (history depth 1 redelivers manifest and current state); an
   application restart bumps the epoch, prompting the UI to re-read the manifest
   and re-validate the hash. The connector's health state machine reports local
   publish health only. Trust is OS- and iceoryx2-mediated in v1.

.. req:: Mandatory SystemViewModel heartbeat with epoch
   :id: REQ_0879
   :status: draft
   :satisfies: FEAT_0096

   The connector shall always publish a ``SystemViewModel`` carrying a
   monotonic counter that advances every publisher-pump tick and a process
   ``epoch`` that uniquely identifies the application process instance. This
   heartbeat shall be the canonical "application alive and pump running"
   signal, distinguishable from a static-but-live ViewModel, and shall be
   exempt from the zero-subscriber skip (:need:`REQ_0862`).

.. req:: Per-ViewModel staleness from the envelope
   :id: REQ_0880
   :status: draft
   :satisfies: FEAT_0096

   The client shall be able to compute per-ViewModel staleness from the
   envelope ``timestamp_ns`` and ``sequence_number`` (:need:`REQ_0202` /
   :need:`REQ_0203`) carried on every publish, so a frozen or absent ViewModel
   can be visually distinguished from a fresh one.

.. req:: UI restart is stateless
   :id: REQ_0881
   :status: draft
   :satisfies: FEAT_0096

   A UI that exits and relaunches shall recover with no application
   involvement: history-depth-1 delivery (:need:`REQ_0856` / :need:`REQ_0872`)
   redelivers the current manifest and the current value of every subscribed
   ViewModel on reconnect. No resync handshake shall be required.

.. req:: Application restart bumps epoch and triggers rebind
   :id: REQ_0882
   :status: draft
   :satisfies: FEAT_0096

   On application restart the process ``epoch`` (:need:`REQ_0879`) shall
   change. A client observing an epoch change shall re-read the manifest and
   re-validate the contract hash (:need:`REQ_0874`), rebinding normally on a
   match and entering read-only fallback (:need:`REQ_0876`) on a mismatch.
   In-flight non-idempotent commands at the epoch boundary shall be handled per
   :need:`REQ_0868`.

.. req:: Connector health reflects local publish health
   :id: REQ_0883
   :status: draft
   :satisfies: FEAT_0096

   The connector's ``Connector`` health state machine (:need:`REQ_0231`) shall
   report local publishing health — pump running, publish backpressure or
   drops — rather than the liveness of any remote peer, since the UI connector
   has no bus partner. Subscriber presence or absence shall not by itself be a
   health fault.

.. req:: OS-mediated trust for v1
   :id: REQ_0884
   :status: draft
   :satisfies: FEAT_0096

   v1 shall rely on operating-system and iceoryx2 access control for the trust
   boundary and shall not implement application-level authentication or role
   separation. The documentation shall state explicitly that command authority
   is granted to any local process able to open the connector's services.
   Capability tokens or read-only/control roles are deferred to a later
   revision.
