EtherCAT network-config codegen — architecture (arc42)
======================================================

Architecture documentation for the EtherCAT network-config codegen
toolchain (see :doc:`../../requirements/ethercat-netcfg/index`), structured per
the arc42 template and encoded with sphinx-needs using the useblocks
"x-as-code" arc42 conventions.

Each decision ``:refines:`` the requirement or capability-cluster
feature it constrains, so the trace from design rationale to shall-clause
is preserved end-to-end. The full grilling trail behind these decisions
lives in
``docs/superpowers/specs/2026-05-31-ethercat-netcfg-codegen-design.md``.

.. contents:: Sections
   :local:
   :depth: 1

----

1. Introduction and goals
-------------------------

The toolchain's reason-to-exist is **moving the EtherCAT bus wiring out
of hand-written Rust constants and into a validated, build-time-compiled
config file**, without disturbing the connector runtime. Today every
``examples/ethercat-*`` ``main.rs`` hand-writes ``SUBDEV`` addresses,
bit offsets, and routings, with a comment warning that the configured
station address shifts whenever the topology changes. This toolchain
compiles a ``network.yaml`` down to the exact ``&'static`` tables those
files write by hand, computing the derived parts (addresses, working
counters) and validating the whole topology.

.. toctree::
   :maxdepth: 2

   building-blocks
   decisions
