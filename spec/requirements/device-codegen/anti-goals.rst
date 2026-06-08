Anti-goals and cross-cutting traceability
=========================================

The following requirements are explicitly **rejected** — captured for
the record so future readers see what the toolchain deliberately does
not do, and why. Each rejected requirement ``:satisfies:``
:need:`FEAT_0050` to keep the umbrella's traceability complete.

.. req:: NO CAN / CANopen / EDS support in this round
   :id: REQ_0590
   :status: rejected
   :satisfies: FEAT_0050

   The toolchain shall **not** include a CAN parser, a CANopen
   runtime trait, an EDS / XDD reader, or a SocketCAN backend.
   CANopen and EtherCAT's CoE share the Object Dictionary
   semantics, but transport semantics diverge (cyclic PDI vs
   event-driven frames). A follow-on spec extracts a shared
   ``fieldbus-od-core`` IR once a concrete CANopen device is in
   scope; see :need:`ADR_0073`.

.. req:: NO proc-macro front-end
   :id: REQ_0591
   :status: rejected
   :satisfies: FEAT_0050

   The toolchain shall **not** offer an ``esi_device!("EL3001.xml")``
   proc-macro form. The IDE-discoverability gain does not justify
   the doubled codegen surface or the worse compile-time profile
   for what is effectively a one-time generation step per device set.
   ``cargo esi expand`` (:need:`REQ_0550`) covers the inspection
   use case.

.. req:: NO unification of EtherCAT and CANopen runtime traits
   :id: REQ_0592
   :status: rejected
   :satisfies: FEAT_0050

   When CANopen support is added in a follow-on spec, the runtime
   trait family shall **not** be merged with ``EsiDevice`` /
   ``EsiConfigurable``. EtherCAT's cyclic-bit-buffer model and
   CANopen's event-driven-frame model are different transport
   semantics; forcing them into one trait would leak a fake
   "process image" into CANopen and mis-model event-triggered
   TPDOs.

.. req:: NO runtime XML parsing
   :id: REQ_0593
   :status: rejected
   :satisfies: FEAT_0050

   The toolchain shall **not** parse ESI XML at application
   runtime. All XML parsing happens at build time in
   ``ethercat-esi-build`` or in the CLI tools. Consumers of the
   generated modules shall not need to ship XML files alongside
   their binary.

.. req:: NO modification of taktora-connector-ethercat runtime
   :id: REQ_0594
   :status: rejected
   :satisfies: FEAT_0050

   This spec shall **not** require any change to the runtime
   contracts of :need:`FEAT_0041` "EtherCAT reference connector".
   The connector consumes ``EsiDevice`` through a thin adapter (see
   :need:`BB_0066`); it does not become aware of XML or codegen.

.. req:: NO automatic vendor library scraping
   :id: REQ_0595
   :status: rejected
   :satisfies: FEAT_0050

   The toolchain shall **not** download, scrape, or otherwise
   fetch ESI XML from vendor websites or update servers. ESI files
   are inputs the user drops into a ``esi/`` directory; provenance
   is the user's responsibility.

----

Cross-cutting traceability
--------------------------

Every requirement in this chapter (excluding rejected anti-goals)
carries a ``:satisfies:`` link to its capability-cluster feat; every
cluster feat ``:satisfies:`` :need:`FEAT_0050`. Architectural
specifications refining these requirements are emitted in
:doc:`../../architecture/device-codegen/index`. Verification artefacts are
emitted in :doc:`../../verification/device-codegen/index`.

.. needtable::
   :types: feat
   :filter: id >= "FEAT_0050" and id <= "FEAT_0059"
   :columns: id, title, status, satisfies
   :show_filters:

.. needtable::
   :types: req
   :filter: id >= "REQ_0500" and id <= "REQ_0599"
   :columns: id, title, status, satisfies
   :show_filters:
