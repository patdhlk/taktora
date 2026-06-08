Zenoh reference connector
=========================

A third concrete connector instantiating the framework's contracts. This
parent feature ``:satisfies:`` :need:`FEAT_0030`; its three capability
clusters — pub/sub (:need:`FEAT_0043`), queries (:need:`FEAT_0044`), and
session topology and health (:need:`FEAT_0045`) — each ``:satisfies:`` it,
on their own pages (see the toctree).

.. feat:: Zenoh reference connector
   :id: FEAT_0042
   :status: open
   :satisfies: FEAT_0030

   A third concrete connector instantiating the framework's contracts:
   ``zenoh``-backed plugin and gateway with bidirectional pub/sub and
   queries. The session topology is configurable between peer and
   client modes; reconnect is delegated to the Zenoh session itself
   (stack-internal posture mirroring :need:`REQ_0235`). Queries are
   exposed via concrete methods on ``ZenohConnector`` only — the
   shared ``Connector`` trait is not modified. The gateway owns one
   ``zenoh::Session`` and runs Zenoh's async callbacks on a tokio
   sidecar contained inside ``taktora-connector-zenoh``. Linux, macOS,
   and Windows are supported host operating systems.

.. toctree::
   :maxdepth: 2

   zenoh-pubsub
   zenoh-queries
   zenoh-session-topology
