Device-driver codegen — verification
====================================

Test cases verifying the device-driver codegen toolchain. Each
``test`` directive ``:verifies:`` one or more requirements from
:doc:`../../requirements/device-codegen/index` (or building blocks from
:doc:`../../architecture/device-codegen/index`).

The toolchain is build-time only — there are no cyclic-runtime
integration tests beyond what :doc:`../connector` already covers for
:need:`FEAT_0041`. The verification surface here is therefore
heavier on snapshot / golden-file / property tests than on
multi-process integration.

The test cases are grouped by area (see the toctree): the seven
crate-layer areas mirroring the requirement feats — parser, codegen /
IR, ethercrab backend snapshots, runtime trait surface, build helper,
CLI, and EEPROM verifier — and a final cross-cutting page carrying the
reproducibility and traceability checks.

.. toctree::
   :maxdepth: 2

   esi-parser
   codegen-ir
   ethercrab-backend
   runtime-trait
   build-helper
   cli
   eeprom-diff
   cross-cutting
