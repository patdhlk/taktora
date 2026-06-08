Logging — DLT base library with swappable backends
==================================================

Requirements for the workspace-wide logging facade and its default DLT
backend. The chapter introduces two new crates — ``taktora-log`` (the
facade) and ``taktora-log-dlt`` (the DLT backend) — and documents how
they coexist with the existing ``taktora-executor-tracing``.

The design rationale, alternatives considered, and reference deployment
context (COVESA dlt-daemon, AUTOSAR R20-11) live in the companion design
doc ``docs/superpowers/specs/2026-05-18-taktora-log-dlt-design.md``
in the repository root.

The umbrella is split into eight capability-cluster sub-features. Each
sub-feature ``:satisfies:`` an umbrella; each ``req`` ``:satisfies:``
exactly one capability-cluster feature. Each sub-feature has its own
page (see the toctree): the facade (:need:`FEAT_0071`), the
backend-swap surface (:need:`FEAT_0073`), the tracing bridge
(:need:`FEAT_0078`), the DLT backend (:need:`FEAT_0072`), structured
fields (:need:`FEAT_0074`), runtime log-level control
(:need:`FEAT_0075`), the offline ring buffer (:need:`FEAT_0076`), and
the console dev fallback (:need:`FEAT_0077`).

Top-level umbrella
------------------

.. feat:: Shared logging base library
   :id: FEAT_0070
   :status: approved

   A workspace-wide logging surface used by every taktora crate and by
   any downstream connector. The umbrella satisfies two competing
   forces simultaneously:

   1. **Vehicle integrators want DLT.** The base library must speak
      AUTOSAR Diagnostic Log and Trace natively to a co-located
      COVESA ``dlt-daemon`` so taktora's events surface in the same
      DLT Viewer / dlt-tui / backend-upload pipeline as everything
      else on the ECU.
   2. **Non-vehicle integrators do not want DLT.** Bench rigs, dev
      machines, CI, and third-party experiments must be able to swap
      DLT for ``log4rs`` / ``env_logger`` / a bespoke logger without
      touching any caller site.

   The resolution is to commit to the rust-native ``log`` crate as the
   workspace logging facade (per :need:`CON_0024`) and ship a DLT
   *backend* behind it (per :need:`FEAT_0072`). This mirrors the
   ``embassy-rs/embassy`` posture for std targets — ``log`` is the
   facade, the backend is chosen at process init.

   The umbrella decomposes into the capability clusters below.

Requirements at a glance
------------------------

.. needtable::
   :columns: id, title, status, satisfies
   :show_filters:
   :filter: "FEAT_0070" in satisfies or "FEAT_0071" in satisfies or "FEAT_0072" in satisfies or "FEAT_0073" in satisfies or "FEAT_0074" in satisfies or "FEAT_0075" in satisfies or "FEAT_0076" in satisfies or "FEAT_0077" in satisfies or "FEAT_0078" in satisfies

.. toctree::
   :maxdepth: 2

   facade
   backend-swap
   tracing-bridge
   dlt-backend
   structured-fields
   log-level-control
   ring-buffer
   console-fallback
