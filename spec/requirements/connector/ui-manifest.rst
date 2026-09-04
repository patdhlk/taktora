Manifest, schema and discovery
==============================

The language-neutral contract: how a UI in any language discovers what
ViewModels and commands exist, learns their layout, and detects an
incompatible build. This cluster ``:satisfies:`` :need:`FEAT_0092`.

.. feat:: Manifest, schema and discovery
   :id: FEAT_0095
   :status: implemented
   :satisfies: FEAT_0092

   The application publishes a single self-describing manifest per instance
   on a well-known, instance-namespaced service. The manifest enumerates every
   ViewModel and command — names, field schemas (drawn from the closed POD
   type set), command signatures, kinds, and idempotent flags — and carries a
   contract hash. A UI reads the manifest, validates the hash, and binds
   dynamically; it never guesses service names. Hash mismatch fails closed
   (read-only inspect, commands disabled).

.. req:: Single instance-namespaced manifest service
   :id: REQ_0872
   :status: implemented
   :satisfies: FEAT_0095
   :links: BB_0046, TEST_0880

   The application shall publish exactly one manifest on a well-known iceoryx2
   service per application instance, with publisher history depth 1 so a
   late-joining UI receives the current manifest immediately. Every service
   the connector creates (manifest, ViewModels, commands) shall be prefixed by
   an instance namespace identifier, configurable via the connector builder
   and defaulting to the process name, so multiple taktora applications can
   coexist on one host without collision.

.. req:: Manifest enumerates all services, schemas and signatures
   :id: REQ_0873
   :status: implemented
   :satisfies: FEAT_0095
   :links: BB_0045, TEST_0883

   The manifest shall enumerate, for the instance: every ViewModel and its
   iceoryx2 service name and field schema; every command and its request /
   reply service names, parameter schema, ``kind``, and idempotent flag
   (:need:`REQ_0868`); and every ``CanExecute`` property. The manifest shall
   be the sole place a UI learns service names — UIs shall not construct
   service names by convention, so the naming scheme remains an internal
   detail.

.. req:: Manifest carries a contract hash
   :id: REQ_0874
   :status: implemented
   :satisfies: FEAT_0095
   :links: BB_0045, TEST_0883

   The manifest shall carry a contract hash computed over the structural
   contract — ViewModel and command names, field names and types, and command
   signatures — so that a client built or generated against one contract can
   detect an incompatible application build by comparing hashes.

.. req:: Closed self-describing schema type system
   :id: REQ_0875
   :status: implemented
   :satisfies: FEAT_0095
   :links: BB_0045, TEST_0883

   The manifest's field-schema descriptors shall express exactly the closed
   POD type set of :need:`REQ_0858` — scalars, fixed-length arrays, inline
   bounded UTF-8 strings (with declared capacity), nested structs, and C-like
   enums (with named discriminants) — so any target language can represent and
   deserialize a ViewModel off the manifest alone. The schema's ``kind``
   enumeration shall reserve an ``Event`` variant for the deferred
   event-stream primitive.

.. req:: Contract-hash mismatch fails closed with read-only fallback
   :id: REQ_0876
   :status: implemented
   :satisfies: FEAT_0095
   :links: BB_0048, TEST_0881

   When a client's expected contract hash does not match the manifest's, the
   client shall refuse to bind in its normal (read-write) mode. It may enter a
   read-only inspect mode that displays ViewModels best-effort by matching
   fields by name, but in that mode all commands shall be disabled. The
   read-only fallback is a compatibility control, not a security control.

.. req:: Multi-application discovery via the iceoryx2 registry
   :id: REQ_0877
   :status: implemented
   :satisfies: FEAT_0095
   :links: BB_0048, TEST_0881

   A UI shall be able to enumerate the live taktora applications on the host by
   scanning the iceoryx2 service registry for services matching the manifest
   naming pattern, then reading each manifest. No central registry process is
   required.

.. req:: Derive macro emits the manifest contribution
   :id: REQ_0878
   :status: implemented
   :satisfies: FEAT_0095
   :links: BB_0047, TEST_0873, TEST_0874, TEST_0883

   The ``#[derive(ViewModel)]`` / command macros shall emit each type's
   manifest contribution — field names and schema descriptors, command
   signatures, kinds, and idempotent flags — so the published manifest and the
   contract hash are generated from the authored Rust types rather than
   maintained by hand.
