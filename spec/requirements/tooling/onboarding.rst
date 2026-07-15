Onboarding golden path
======================

A curated front door for the workspace, so assembling a first application is
a guided path rather than an archaeology problem.

.. feat:: Onboarding golden path + assembly guide
   :id: FEAT_0121
   :status: draft

   **Motivation.** The workspace ships 41 library crates (plus ``*-tests``
   siblings) and no facade — a real application assembles five to eight of
   them by hand. With systematically-but-verbosely named crates, discovery
   is the first-hour problem: a newcomer landing on the README meets a
   by-layer crate table with no entry point and no runnable "this is how the
   pieces fit" reference. The cheapest, highest-leverage fix is a single
   golden-path example plus a short assembly guide that names the canonical
   stack and points at it.

   **Scope.** A runnable, hardware-free golden-path example
   (``examples/mqtt-zenoh-bridge`` — the executor driving an MQTT and a
   Zenoh connector on one executor); a two-tier assembly guide (a README
   "Start here" section naming the ~7-crate stack, plus the example's own
   README as the deep walkthrough); documentation positioning that makes the
   ESI+netcfg codegen path the default EtherCAT integration story and manual
   PDI routing the escape hatch; and executor API-clarity cleanup
   (disambiguating three near-synonym type pairs, removing the
   ``std::ops::ControlFlow`` name shadow, and dropping stale ``html_root_url``
   doc metadata).

   **Non-goals.** A ``taktora`` umbrella / facade crate is **deferred**
   (O2): re-exporting 41 independently-versioned pre-1.0 crates behind one
   semver surface is a standing liability, revisited only once the curated
   ~7-crate subset proves stable enough to carry that surface — see the
   forthcoming front-door architecture decision. A codegen-on-``MockBusDriver``
   EtherCAT example is **deferred** (O1): it would make "codegen is the
   default" runnable without hardware, but generating a real vendor driver
   only to loop its own outputs back to its inputs is of questionable
   fidelity; the positioning stays docs-only for now.

.. req:: Hardware-free two-connector golden-path example
   :id: REQ_1002
   :status: draft
   :satisfies: FEAT_0121

   The workspace shall provide a runnable example that drives two connectors
   (MQTT ingress and Zenoh egress) on a single executor with no external
   broker, router, or fieldbus hardware, exiting with a deterministic
   summary of frames ingested, bridged, and egressed.

.. req:: Uniform channel seam across protocols
   :id: REQ_1003
   :status: draft
   :satisfies: FEAT_0121

   The golden-path example shall move payloads across both connectors
   through the same ``ChannelReader`` / ``ChannelWriter`` API, demonstrating
   that the connector seam is protocol-independent.

.. req:: README "Start here" assembly pointer
   :id: REQ_1004
   :status: draft
   :satisfies: FEAT_0121

   The top-level README shall carry a "Start here" section that names the
   canonical application crate stack and links the golden-path example as the
   entry point, positioned ahead of the by-layer crate tables.

.. req:: Golden-path example walkthrough
   :id: REQ_1005
   :status: draft
   :satisfies: FEAT_0121

   The golden-path example shall ship its own README documenting the crate
   stack it assembles and walking the wiring in code order.

.. req:: EtherCAT codegen documented as the default path
   :id: REQ_1006
   :status: draft
   :satisfies: FEAT_0121

   EtherCAT documentation shall present the ESI + netcfg build-time codegen
   toolchain as the default integration path and frame manual PDI bit-slice
   routing as the escape hatch, without altering the manual routing API.

.. req:: Disambiguated executor type pairs
   :id: REQ_1007
   :status: draft
   :satisfies: FEAT_0121

   The executor's near-synonym public type pairs
   (``ItemFlow`` / ``ExecuteResult``, ``FaultState`` / ``ExecutorFaultState``,
   ``Observer`` / ``ExecutionMonitor``) shall each carry reciprocal
   documentation cross-references stating how each differs from its sibling.

.. req:: No standard-library control-flow name shadow
   :id: REQ_1008
   :status: draft
   :satisfies: FEAT_0121

   The executor's post-item control-flow enum shall not reuse the name
   ``ControlFlow`` (which shadows ``std::ops::ControlFlow``); it shall be
   named distinctly so an errant import fails to compile rather than
   type-checking against the wrong type.

.. req:: No stale documentation-root metadata
   :id: REQ_1009
   :status: draft
   :satisfies: FEAT_0121

   Published executor and logging crates shall not carry
   ``#![doc(html_root_url = ...)]`` attributes pinned to a stale version;
   cross-crate doc links shall be left to docs.rs inference.
