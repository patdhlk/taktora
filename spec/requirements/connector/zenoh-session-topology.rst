Zenoh session topology and health
=================================

The Zenoh-specific session and observability surface. This cluster
``:satisfies:`` :need:`FEAT_0042`.

.. feat:: Zenoh session topology and health
   :id: FEAT_0045
   :status: open
   :satisfies: FEAT_0042

   The Zenoh-specific session and observability surface — peer-vs-
   client mode configuration, scout/locator wiring, and the
   stack-internal reconnect posture. Health-event semantics inherit
   from :need:`FEAT_0034` and re-affirm :need:`REQ_0235` (stack-
   internal reconnect emits health events without
   ``ReconnectPolicy``).

.. req:: Zenoh session mode is a config knob
   :id: REQ_0440
   :status: implemented
   :satisfies: FEAT_0045
   :links: BB_0042, TEST_0308, TEST_0312, TEST_0313

   ``ZenohConnectorOptions::mode`` shall accept the values
   ``SessionMode::{Peer, Client, Router}`` and shall default to
   ``Peer``. The gateway shall translate this knob into the
   corresponding ``zenoh::Config`` field before calling
   ``zenoh::open``.

.. req:: NO ReconnectPolicy on Zenoh session loss
   :id: REQ_0441
   :status: rejected
   :satisfies: FEAT_0045

   The Zenoh connector shall **not** use
   :need:`REQ_0232` ``ReconnectPolicy`` on session loss. Zenoh's
   own scout / reconnect machinery owns the retry; the connector
   merely emits ``HealthEvent`` on every observed transition
   between ``ConnectorHealth`` variants (mirrors :need:`REQ_0235`
   for tonic/gRPC).

.. req:: HealthEvent emitted on every Zenoh session transition
   :id: REQ_0442
   :status: implemented
   :satisfies: FEAT_0045
   :links: BB_0042, TEST_0308

   Every transition of the Zenoh session between alive and closed
   states observed by the gateway (including the initial
   ``Connecting → Up`` and any subsequent re-bringup driven by
   Zenoh's own retry) shall emit one ``HealthEvent`` on the
   connector's health channel (re-affirms :need:`REQ_0234`).

.. req:: Connect and listen locators surfaced to zenoh::Config
   :id: REQ_0443
   :status: open
   :satisfies: FEAT_0045

   ``ZenohConnectorOptions::connect`` and
   ``ZenohConnectorOptions::listen`` shall be carried to
   ``zenoh::Config`` verbatim before ``zenoh::open``. Validation of
   locator URIs is delegated to ``zenoh`` (the connector neither
   parses nor canonicalises them).

.. req:: zenoh-integration cargo feature gates the real zenoh dep
   :id: REQ_0444
   :status: implemented
   :satisfies: FEAT_0045
   :links: BB_0040, TEST_0310

   The real ``zenoh`` crate shall be an optional dependency of
   ``taktora-connector-zenoh``, activated only by a default-off
   ``zenoh-integration`` cargo feature (mirrors :need:`BB_0030`'s
   ``bus-integration`` posture). Availability of
   ``MockZenohSession`` and the connector framework types in the
   default build is covered separately by :need:`REQ_0445`.

.. req:: MockZenohSession ships unfeature-gated
   :id: REQ_0445
   :status: implemented
   :satisfies: FEAT_0045
   :links: BB_0040, TEST_0302, TEST_0310

   ``MockZenohSession`` — an in-process pub/sub + query loopback
   implementation of the ``ZenohSessionLike`` trait — shall ship in
   the default build, not gated by ``zenoh-integration``. It exists
   so that the Layer-1 (pure-logic) test pyramid can exercise the
   full envelope ↔ session ↔ envelope hop without depending on the
   real ``zenoh`` crate.

.. req:: Linux, macOS, and Windows are supported host operating systems
   :id: REQ_0446
   :status: implemented
   :satisfies: FEAT_0045
   :links: BB_0040, TEST_0311

   The Zenoh connector shall support Linux, macOS, and Windows as
   host operating systems for both plugin and gateway (broader than
   :need:`REQ_0325`'s Linux-only EtherCAT posture, because Zenoh has
   no OS-specific socket requirement comparable to ``CAP_NET_RAW``).
