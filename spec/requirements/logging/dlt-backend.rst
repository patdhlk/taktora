DLT backend — taktora-log-dlt
=============================

The default backend behind the facade: a pure-Rust AUTOSAR Classic DLT
R20-11 encoder plus a daemon client, usable through :need:`FEAT_0071`
or standalone as a ``log::Log``.

.. feat:: taktora-log-dlt DLT-protocol backend
   :id: FEAT_0072
   :status: implemented
   :satisfies: FEAT_0070

   A pure-Rust DLT backend (``taktora-log-dlt``) that implements both
   ``LogSink`` (for use through :need:`FEAT_0071`) and ``log::Log``
   directly (for standalone use without the facade crate). It encodes
   AUTOSAR Classic DLT R20-11 messages via the ``dlt-core`` crate
   (esrlabs), owns a Unix-domain-socket or TCP client to a
   co-located ``dlt-daemon``, and threads the producer hot path
   through a bounded queue and a single background flusher (see
   :need:`ARCH_0072`).

.. req:: AUTOSAR Classic DLT R20-11 encoding via dlt-core
   :id: REQ_0806
   :status: implemented
   :satisfies: FEAT_0072
   :links: BB_0091, TEST_0806

   The DLT backend shall encode every emitted record as an AUTOSAR
   Classic DLT R20-11 message — Storage Header + Standard Header +
   Extended Header + payload — using the ``dlt-core`` crate
   (``esrlabs/dlt-core``) as the encoder. Hand-written byte assembly
   is rejected; schema maintenance lives in the encoder crate.

.. req:: UDS (default) and TCP transports to a local dlt-daemon
   :id: REQ_0807
   :status: implemented
   :satisfies: FEAT_0072
   :links: BB_0092, TEST_0807

   The DLT backend shall support delivery to a co-located
   ``dlt-daemon`` via a Unix-domain socket (default) and via TCP
   (opt-in). The transport shall be selectable through the
   ``taktora-log-dlt`` builder. The backend shall not start, supervise,
   restart, or reconfigure the daemon — that responsibility is the
   integrator's (see :need:`AOU_0010`).

.. req:: 4-character DLT App ID and Context ID per emitting crate
   :id: REQ_0808
   :status: implemented
   :satisfies: FEAT_0072
   :links: BB_0091, TEST_0808

   Each taktora crate that emits log records via ``taktora-log-dlt``
   shall declare a 4-character DLT App ID and one or more 4-character
   Context IDs at init time. taktora reserves the ``TK*`` prefix for
   its own crates (working straw-man: ``TKEX`` taktora-executor,
   ``TKCC`` connector-core, ``TKCH`` connector-host, ``TKZN``
   connector-zenoh, ``TKEC`` connector-ethercat, ``TKCN`` connector-can,
   ``TKCD`` connector-codec, ``TKRP`` replay). Integrators shall pick
   non-``TK*`` IDs (see :need:`AOU_0013`).
