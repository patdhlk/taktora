Crosscutting concepts, deployment, quality, and glossary
========================================================

This page groups the short arc42 tail sections: deployment (§7),
crosscutting concepts (§8), quality requirements (§10), and the glossary
(§12). The decisions pointer (§9) is on :doc:`decisions`; risks (§11)
are on :doc:`risks`.

----

7. Deployment view
------------------

.. architecture:: Toolchain crate placement in workspace
   :id: ARCH_0054
   :status: open
   :refines: FEAT_0050

   All seven toolchain crates live in ``crates/`` alongside the
   existing ``taktora-connector-ethercat`` and friends. The
   workspace ``Cargo.toml`` adds them to ``members``; pinning
   matches the rest of the workspace (``rust-toolchain.toml`` MSRV
   1.85, edition 2024 per :need:`CON_0003`-style constraint
   tracking).

   No deployment-time changes: the toolchain is a build-time
   artefact. The only runtime consequence is that
   ``taktora-connector-ethercat`` gains a path-dep on
   ``ethercat-esi-rt`` and an internal ``EsiDevice`` adapter.

----

8. Crosscutting concepts
------------------------

The crosscutting axes are owned by section 1 (quality goals) and
section 2 (constraints). The two persistent runtime concepts —
the ``EsiDevice`` trait and the per-device ``Identity`` const — both
live in :need:`BB_0063` and are referenced from generated code,
adapters, and dispatch registries alike. They are the contract
the rest of the toolchain orbits.

----

10. Quality requirements
------------------------

The quality goals in section 1 (:need:`QG_0010` through
:need:`QG_0013`) define the qualities. The verification artefacts
in :doc:`../../verification/device-codegen/index` exercise each one.

----

12. Glossary
------------

.. term:: ESI
   :id: GLOSS_0020
   :status: open

   EtherCAT Slave Information — an XML file describing a single
   EtherCAT device's identity, PDOs, mailbox, distributed clocks,
   and object dictionary. Schema is published by ETG
   (``EtherCATInfo.xsd``).

.. term:: SII
   :id: GLOSS_0021
   :status: open

   Slave Information Interface — the on-device EEPROM that
   carries a binary subset of the ESI data, readable over the
   EtherCAT bus by the master. ``ethercat-esi-verify`` cross-checks
   ESI XML against captured SII ``.bin`` dumps.

.. term:: PDO
   :id: GLOSS_0022
   :status: open

   Process Data Object — a fixed-length packed set of OD entries
   exchanged on every EtherCAT cycle. RxPDO = master → device
   (outputs); TxPDO = device → master (inputs).

.. term:: CoE
   :id: GLOSS_0023
   :status: open

   CANopen over EtherCAT — mailbox protocol carrying CANopen SDO
   writes (e.g. PDO assignment writes to 0x1C12 / 0x1C13).

.. term:: OD (Object Dictionary)
   :id: GLOSS_0024
   :status: open

   The indexed (16-bit index + 8-bit sub-index) catalogue of
   readable / writable objects on a CANopen or CoE device.
   Inherited by EtherCAT from CANopen (CiA 301).

.. term:: InitCmd
   :id: GLOSS_0025
   :status: open

   An SDO write sequence declared inside an ESI ``<Mailbox>``
   section that must run during a specific state transition
   (typically PRE-OP → SAFE-OP). Carries the bring-up data
   (filter coefficients, scaling values, channel modes) the
   device expects before cyclic operation.
