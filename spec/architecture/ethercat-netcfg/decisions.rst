Architecture decisions
======================

arc42 §9 — the accepted decisions behind the EtherCAT network-config
codegen toolchain (:need:`FEAT_0080`). Each ``arch-decision``
``:refines:`` the parent requirement or feature it serves.

.. contents:: Decisions
   :local:
   :depth: 1

----

.. arch-decision:: Build-time codegen over runtime parsing
   :id: ADR_0092
   :status: open
   :refines: FEAT_0080

   **Context.** "Configure the EtherCAT network with a YAML file" admits
   two architectures: parse the YAML at startup into owned, heap-allocated
   config, or compile it at build time into the ``&'static`` tables the
   connector already consumes. The connector's ``EthercatConnectorOptions``
   holds ``pdo_map: &'static [SubDeviceMap]`` precisely so the mapping
   lives in ``.rodata`` with no per-instance heap; the workspace ships a
   ``taktora-bounded-alloc`` global allocator and an ISO 26262 SEooC
   safety concept.

   **Decision.** The YAML is resolved at **build time**. A ``build.rs``
   step parses, validates, and generates the ``&'static`` tables; the
   runtime is untouched and performs no parsing. The YAML is purely an
   authoring surface for what the examples write by hand.

   **Consequences.** Allocation is known at init, the ``.rodata`` /
   no-runtime-parsing / bounded-alloc invariant holds, and the safety
   argument is undisturbed. The cost is that reconfiguring a deployed
   binary requires a rebuild — accepted, because field reconfiguration
   without recompilation is a non-goal and would reverse the architecture.

.. arch-decision:: Positional addressing; alias and identity as bring-up assertions
   :id: ADR_0093
   :status: open
   :refines: FEAT_0085

   **Context.** A device on an EtherCAT bus can be named three ways: by
   configured station address (``0x1000 + n``, assigned by the master at
   init in bus order — derived, not intrinsic), by station alias (the
   DIP-switch / EEPROM value — intrinsic, survives reordering), or by
   topological position. Hand-written examples encode the configured
   address as a literal, which is the source of all "adjust ``SUBDEV`` if
   your topology differs" fragility.

   **Decision.** The YAML declares devices in **bus order**; codegen
   assigns the configured address as ``0x1000 + n``. ``station_alias`` and
   ``identity`` do not drive addressing — they become **bring-up
   assertions** the runtime verifies. An explicit ``address`` override is
   the rare escape hatch.

   **Consequences.** Integrators never hand-type ``0x1001`` again; they
   reorder list entries. A mis-cabled or swapped bus fails loudly at
   bring-up (identity / alias / working-counter mismatch) instead of
   silently mirroring process data to the wrong terminal.

.. arch-decision:: Local-only ESI resolution; URLs are a vendor-and-pin step
   :id: ADR_0094
   :status: open
   :refines: FEAT_0084

   **Context.** A device entry may reference a vendor ESI file by local
   path or web URL. A live web fetch inside ``build.rs`` would make the
   build non-hermetic: output would depend on what the server returns that
   day, offline / air-gapped / ``cargo``-offline builds would break, and
   "the bus mapping came from a URL at build time" is indefensible in a
   safety audit.

   **Decision.** The build resolves ESI references from **local files
   only**. A web URL is an input to a deliberate ``netcfg fetch`` action
   that downloads once into a vendored directory and records a content
   hash + device revision in a lockfile. The build reads only the local
   pinned copy; a URL with no matching vendored file is a build error.

   **Consequences.** Builds stay hermetic, reproducible, and
   air-gappable, mirroring how ``cargo`` vendors crates. A silently
   re-published vendor ESI surfaces as a lockfile diff (per
   :need:`REQ_0835`) rather than a behaviour change.

.. arch-decision:: Working-counter expectation derived only, never overridden
   :id: ADR_0095
   :status: open
   :refines: FEAT_0085

   **Context.** ``SubDeviceMap`` carries an ``expected_wkc`` whose
   canonical value follows the 0/1/2/3 rule from the mapped PDO directions
   (documented in ``options.rs``). Allowing the YAML to also specify it
   creates two sources of truth that can disagree.

   **Decision.** ``expected_wkc`` is **derived** from the PDO directions
   by codegen. The toolchain offers no override.

   **Consequences.** One source of truth; the value cannot drift from the
   mapping that produced it. A bus whose real working counter diverges
   from the derived expectation is caught at runtime via the existing
   ``wkc::evaluate_wkc`` health path.

.. arch-decision:: One file, one bus; multi-bus distribution deferred
   :id: ADR_0096
   :status: open
   :refines: FEAT_0081

   **Context.** A config could describe one bus or many. Each bus maps to
   one connector, one NIC, and one process image, and carries its own
   compile-time const-generic bounds (``MAX_SUBDEVICES`` / ``MAX_PDI``).

   **Decision.** One ``network.yaml`` describes exactly one bus. Multiple
   buses are multiple files producing multiple generated modules.
   Multi-bus documents and distribution / include across files are a
   **future enhancement**, out of scope for v1.

   **Consequences.** The schema and the const-generic bounds stay simple
   and per-bus, matching the one-NIC-per-example reality. The cost is that
   a large multi-bus installation maintains several files — accepted for
   v1, revisited if a real multi-bus need appears.
