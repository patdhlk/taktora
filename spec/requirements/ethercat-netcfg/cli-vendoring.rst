CLI and ESI vendoring
=====================

CLI and vendoring requirements for :need:`FEAT_0084` — the command-line
surface for inspecting generated output and pinning remote ESI files.

.. feat:: CLI and ESI vendoring
   :id: FEAT_0084
   :status: open
   :satisfies: FEAT_0080

   A command-line surface for inspecting generated output and for the
   deliberate vendor-and-pin action that brings remote ESI files local.

.. req:: Expand subcommand prints generated module
   :id: REQ_0832
   :status: open
   :satisfies: FEAT_0084

   The CLI shall provide a ``netcfg expand`` subcommand that prints the
   generated module to stdout for inspection and diffing, mirroring the
   ESI toolchain's ``cargo esi expand`` (see :need:`ADR_0077`).

.. req:: Fetch subcommand vendors and pins remote ESI
   :id: REQ_0833
   :status: open
   :satisfies: FEAT_0084

   The CLI shall provide a ``netcfg fetch`` subcommand that resolves a
   web-URL ESI reference once, downloads it into a local vendored
   directory, and records its content hash and device revision into a
   lockfile beside the ``network.yaml``.

.. req:: Build resolves ESI from local files only
   :id: REQ_0834
   :status: open
   :satisfies: FEAT_0084

   The build path shall resolve ESI references from local files only. A
   web-URL reference with no matching vendored, pinned local file shall
   be a build error, never a live network fetch. Builds shall be
   hermetic, reproducible, and runnable air-gapped.

.. req:: ESI references pinned by content hash and revision
   :id: REQ_0835
   :status: open
   :satisfies: FEAT_0084

   Every ESI reference shall be pinned by content hash and device
   revision. A mismatch between a resolved ESI file and its pinned hash
   or revision shall be a build error (per :need:`REQ_0837`), so a
   silently re-published vendor file surfaces as a visible diff rather
   than a behaviour change.
