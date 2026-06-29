Anti-goals and cross-cutting traceability
==========================================

The following requirements are explicitly **rejected** — captured for
the record so future readers see what the toolchain deliberately does
not do this round, and why. Each rejected requirement ``:satisfies:``
:need:`FEAT_0110` to keep the umbrella's traceability complete.

.. req:: NO additional frontends this round
   :id: REQ_0958
   :status: rejected
   :satisfies: FEAT_0110

   The toolchain shall **not** ship OMG IDL or ROS 2
   ``.msg``/``.srv`` frontends this round. DBC (:need:`FEAT_0113`) is
   bounded by construction, which makes it the cleanest first proof of
   the description → IR → codegen pipeline. The harder frontends — which
   must actively *reject* unbounded sequences — are net-additive
   follow-ons; they lower onto the same IR (:need:`FEAT_0111`) and reuse
   the same codegen layer (:need:`FEAT_0114`) without an IR break.

.. req:: NO bool or float CAN fields this round
   :id: REQ_0959
   :status: rejected
   :satisfies: FEAT_0110

   ``taktora-idl-codegen-can`` shall **not** emit ``bool`` or
   floating-point signal fields this round; such fields are rejected with
   a backend error. DBC signals lower to integer and enum fields in the
   delivered slice (:need:`REQ_0956`). Admitting ``bool``/float is a
   follow-on that touches only the backend, not the IR or wire runtime.

.. req:: NO J1939 application-layer consumer this round
   :id: REQ_0960
   :status: rejected
   :satisfies: FEAT_0110

   This spec shall **not** deliver a J1939 PGN/SPN consumer of the
   generated message types. The J1939 connector (:need:`FEAT_0098`)
   remains a raw-(re)assembled-bytes transport this round; typed per-PGN
   signal mapping onto the message plane is a foreseen follow-on that
   adds a frontend/consumer, not a change to this toolchain's contracts.

.. req:: NO runtime description parsing
   :id: REQ_0945
   :status: rejected
   :satisfies: FEAT_0110

   The toolchain shall **not** parse ``.dbc`` (or any interface
   description) at application runtime. All parsing and code emission
   happen at build time; a runtime consumer links only against
   ``taktora-idl-wire`` (:need:`FEAT_0112`) and the generated module, and
   ships no description file alongside its binary.

----

Cross-cutting traceability
--------------------------

Every requirement in this chapter (excluding rejected anti-goals)
carries a ``:satisfies:`` link to its capability-cluster feat; every
cluster feat ``:satisfies:`` :need:`FEAT_0110`. Architectural
specifications refining these requirements are emitted in
:doc:`../../architecture/idl/index`. Verification artefacts are emitted
in :doc:`../../verification/idl/index`.

.. needtable::
   :types: feat
   :filter: id >= "FEAT_0110" and id <= "FEAT_0119"
   :columns: id, title, status, satisfies
   :show_filters:

.. needtable::
   :types: req
   :filter: id >= "REQ_0946" and id <= "REQ_0945"
   :columns: id, title, status, satisfies
   :show_filters:
