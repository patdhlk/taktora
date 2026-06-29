Bounded message-type IR
=======================

The shared bounded message-type IR (:need:`BB_0117`): the plane-generic
description of a message — structs, enums, bounded sequences, and
request/reply services — onto which every frontend lowers. The
message-plane twin of ``fieldbus-od-core``.

.. feat:: Bounded message-type IR
   :id: FEAT_0111
   :status: implemented
   :satisfies: FEAT_0110

   A new crate ``taktora-idl-core`` carrying the message-type IR shared
   by every frontend (DBC today; OMG IDL, ROS 2 later). The IR makes
   unboundedness unrepresentable by construction so every type has a
   finite, compile-time-known serialized-length bound.

.. req:: Boundedness is unrepresentable to violate
   :id: REQ_0946
   :status: implemented
   :satisfies: FEAT_0111
   :links: BB_0117, TEST_0928

   Every type the IR can express shall have a finite, statically
   computable maximum serialized length. ``String`` and ``Sequence``
   shall each carry a capacity bound in the type itself, so an unbounded
   string or sequence cannot be constructed. ``Module`` shall expose
   ``max_serialized_len`` (and a per-struct variant) returning that
   bound for any validated module.

.. req:: Structural validation before sizing
   :id: REQ_0947
   :status: implemented
   :satisfies: FEAT_0111
   :links: BB_0117, TEST_0928

   ``Module::validate`` shall reject a module that is not structurally
   sound: a type that references an unknown struct or enum, a duplicate
   type or field name, a recursive (directly or transitively
   self-referential) struct, or a service whose request/response names
   no declared type. ``max_serialized_len`` is defined only for a module
   that has validated.

.. req:: Plane-generic, policy-free IR
   :id: REQ_0948
   :status: implemented
   :satisfies: FEAT_0111
   :links: BB_0117, TEST_0928

   ``taktora-idl-core`` shall name no wire format, no transport, and no
   target language. It shall not depend on ``ethercrab``, ``socketcan``,
   ``proc-macro2``, ``quote``, or any ``taktora-connector-*`` crate.
   Source identifiers shall be carried verbatim; sanitisation to a
   target-language identifier is a codegen concern (:need:`REQ_0954`),
   not an IR one.
