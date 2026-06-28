Message-plane interface-description codegen — verification
==========================================================

Test cases verifying the message-plane interface-description codegen
toolchain. Each ``test`` directive ``:verifies:`` one or more
requirements from :doc:`../../requirements/idl/index` (or building blocks
from :doc:`../../architecture/idl/index`).

The toolchain is build-time only. The wire runtime
(``taktora-idl-wire``) carries no unit tests of its own; its primitives
are verified through the generated code in the
``taktora-idl-codegen-can-tests`` harness, which packs and unpacks real
signals and asserts the bytes. The verification surface is therefore
heavier on round-trip and golden-placement tests than on multi-process
integration.

The test cases are grouped by area (see the toctree): IR-core unit tests,
DBC frontend tests, codegen tests, and the CAN backend / generated-code
round-trip tests.

.. toctree::
   :maxdepth: 2

   ir-core
   dbc-frontend
   codegen
   can-backend
