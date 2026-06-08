Anti-goals and cross-cutting traceability
==========================================

The following requirements are explicitly **rejected** — captured for
the record so future readers see what the toolchain deliberately does
not do, and why. Each rejected requirement ``:satisfies:``
:need:`FEAT_0060` to keep the umbrella's traceability complete.

.. req:: NO DCF support this round
   :id: REQ_0790
   :status: rejected
   :satisfies: FEAT_0060

   The toolchain shall **not** parse DCF (Device Configuration
   File) inputs this round. EDS describes a device's *shape*; DCF
   describes per-node *configuration* (chosen RPDO/TPDO mapping,
   node-id, SDO-write-at-bringup values). DCF support is a
   follow-on spec; adding it later does not require an IR break
   because the EDS IR already carries the shape DCF references.

.. req:: NO CAN-FD payload support in PdoOut
   :id: REQ_0791
   :status: rejected
   :satisfies: FEAT_0060

   ``PdoOut::payload`` shall **not** support CAN-FD's 64-byte
   payload this round. Lifting ``heapless::Vec<u8, 8>`` to a
   const-generic capacity (``heapless::Vec<u8, N>``) is a follow-on.
   See :need:`ADR_0084`.

.. req:: NO proc-macro front-end
   :id: REQ_0792
   :status: rejected
   :satisfies: FEAT_0060

   The toolchain shall **not** offer a
   ``canopen_device!("foo.eds")`` proc-macro form. The
   IDE-discoverability gain does not justify the doubled codegen
   surface or the worse compile-time profile. ``cargo eds expand``
   (:need:`REQ_0770`) covers the inspection use case. Mirrors
   :need:`REQ_0591`.

.. req:: NO unification of EtherCAT and CANopen runtime traits
   :id: REQ_0793
   :status: rejected
   :satisfies: FEAT_0060

   ``CanOpenDevice`` shall **not** be merged with
   ``EsiDevice`` / ``EsiConfigurable``. EtherCAT's cyclic-bit-buffer
   model and CANopen's event-driven-frame model are different
   transport semantics; forcing them into one trait would leak a
   fake "process image" into CANopen and mis-model event-triggered
   TPDOs. This requirement closes the loop on :need:`REQ_0592` by
   delivering the separate trait family the rejection reserved.

.. req:: NO runtime EDS parsing
   :id: REQ_0794
   :status: rejected
   :satisfies: FEAT_0060

   The toolchain shall **not** parse EDS files at application
   runtime. All EDS parsing happens at build time in
   ``canopen-eds-build`` or in the CLI tools. Consumers of the
   generated modules shall not need to ship EDS files alongside
   their binary. Mirrors :need:`REQ_0593`.

.. req:: NO modification of taktora-connector-can runtime
   :id: REQ_0795
   :status: rejected
   :satisfies: FEAT_0060

   This spec shall **not** require any change to the runtime
   contracts of :need:`FEAT_0046` "CAN reference connector". A
   thin adapter that maps any ``CanOpenDevice`` into the
   connector's frame plumbing is a follow-on spec; this umbrella
   stops at producing typed devices that implement
   ``canopen-eds-rt`` traits. Mirrors :need:`REQ_0594`.

.. req:: NO automatic vendor library scraping
   :id: REQ_0796
   :status: rejected
   :satisfies: FEAT_0060

   The toolchain shall **not** download, scrape, or otherwise
   fetch EDS files from vendor websites or update servers. EDS
   files are inputs the user drops into an ``eds/`` directory;
   provenance is the user's responsibility. Mirrors
   :need:`REQ_0595`.

.. req:: NO live-bus verifier this round
   :id: REQ_0797
   :status: rejected
   :satisfies: FEAT_0060

   ``canopen-eds-verify`` shall **not** open a SocketCAN
   interface, send live SDO upload requests, or otherwise touch a
   real bus. Verification is strictly offline — EDS file vs.
   captured JSON dump (:need:`REQ_0780`). Live verification
   belongs in the follow-on ``taktora-connector-can`` adapter spec
   where the bus is already at hand.

----

Cross-cutting traceability
--------------------------

Every requirement in this chapter (excluding rejected anti-goals)
carries a ``:satisfies:`` link to its capability-cluster feat; every
cluster feat ``:satisfies:`` :need:`FEAT_0060`. Architectural
specifications refining these requirements are emitted in
:doc:`../../architecture/canopen-codegen/index`. Verification artefacts are
emitted in :doc:`../../verification/canopen-codegen/index`.

.. needtable::
   :types: feat
   :filter: id >= "FEAT_0060" and id <= "FEAT_0069"
   :columns: id, title, status, satisfies
   :show_filters:

.. needtable::
   :types: req
   :filter: id >= "REQ_0700" and id <= "REQ_0799"
   :columns: id, title, status, satisfies
   :show_filters:
