EtherCAT network-config codegen — verification
===============================================

Test cases verifying the EtherCAT network-config codegen toolchain.
Each ``test`` directive ``:verifies:`` one or more requirements from
:doc:`../../requirements/ethercat-netcfg/index`.

The toolchain is build-time only. As with :doc:`../device-codegen/index`, the
verification surface is heavier on snapshot / golden-file / property
tests and build-script behaviour than on multi-process integration; the
generated tables are exercised end-to-end against real hardware by the
``examples/ethercat-*`` integration examples, which this page does not
duplicate.

.. toctree::
   :maxdepth: 2

   parser-ir
   codegen
   build-glue
   cli-vendoring
   validation
