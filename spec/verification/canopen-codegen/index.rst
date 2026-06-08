CANopen device-driver codegen — verification
============================================

Test cases verifying the CANopen device-driver codegen toolchain.
Each ``test`` directive ``:verifies:`` one or more requirements from
:doc:`../../requirements/canopen-codegen/index` (or building blocks /
quality goals from :doc:`../../architecture/canopen-codegen/index`).

The toolchain is build-time only — there are no cyclic-runtime
integration tests beyond what :doc:`../connector/index` already covers for
:need:`FEAT_0046`. The verification surface here is therefore
heavier on snapshot / golden-file / property tests than on
multi-process integration. Mirrors the structure of
:doc:`../device-codegen/index` so reviewers can read both verification
pages 1:1.

The test cases are grouped by area (see the toctree): the eight
crate-layer areas mirroring the requirement feats — OD-core unit tests,
EDS parser tests, codegen / IR tests, taktora backend snapshot tests,
runtime trait surface tests, build helper tests, CLI tests, and
verifier tests — and a final cross-cutting page carrying the
reproducibility and traceability checks.

.. toctree::
   :maxdepth: 2

   od-core
   eds-parser
   codegen-ir
   can-backend
   runtime-trait
   build-helper
   cli
   sdo-verify
   cross-cutting
