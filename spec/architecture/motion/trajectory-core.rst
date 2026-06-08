Allocation-free trajectory core
===============================

Architecture decisions for the pure algorithmic layer
``taktora-motion-core`` (the FEAT_0091 trajectory core).

Architecture decisions
----------------------

.. arch-decision:: f64 + libm in the trajectory core; integer increments at the drive
   :id: ADR_0099
   :status: accepted
   :refines: FEAT_0091
   :links: REQ_0060

   **Context.** Position in a motion core is *accumulated* over millions
   of cycles (at a 2 ms cycle, ~1.8 M cycles/hour), so the numeric
   representation must not drift and must stay within the bounded-time
   dispatch budget (:need:`REQ_0060`, :need:`FEAT_0017`). Three
   candidates were weighed: ``f32``, ``f64`` (the PLC ``LREAL``
   baseline), and a base-10 fixed-precision decimal
   (``rust_decimal``-style).

   * ``f32``'s 24-bit mantissa (~7 significant digits) drifts visibly on
     a fine-resolution axis over hours — the original reason to prefer a
     wider type.
   * A base-10 decimal is *exact* for ``+ − ×`` of decimal quantities,
     but motion math is inherently irrational: the very first profile
     computes ``v_peak = sqrt(a·d)``, and S-curve durations, the
     flying-saw quintic, cam polynomials, and any sinusoidal cam law all
     round regardless. Decimal's exactness evaporates at the first
     ``sqrt`` while its cost remains: 128-bit *software* arithmetic is
     ~10–100× slower than hardware ``f64`` and its per-op cost varies
     with operand scale, working directly against *bounded*-time
     dispatch.

   **Decision.** ``taktora-motion-core`` represents all kinematic
   quantities as ``f64`` in abstract engineering units (position in user
   units, velocity units/s, acceleration units/s²). The ``f64``
   transcendentals absent from ``core`` under ``no_std`` (``sqrt``,
   ``fabs``, ``fmod``) are taken from ``libm``, which is bit-reproducible
   across host and target — keeping trajectory output identical on the
   dev box and the deployment board (required for the differential
   oracle and HIL replay). The genuinely-exact, drift-free part of the
   pipeline is the **drive boundary**: CiA 402 commands position in
   integer encoder increments, so the ``units → increments`` conversion
   is integer arithmetic that rounds exactly once, in the glue layer
   (``taktora-motion``), not in the core.

   Unlike the ESI/OD-core crates, which adopted a ``std`` baseline for
   ``thiserror`` and located errors (:need:`ADR_0097`), the trajectory
   core stays ``no_std`` and allocation-free: it needs no error derive
   (one hand-written ``core::error::Error`` enum, surfaced only at
   command-construction time) and the hot path is infallible.

   **Consequences.** Drift over realistic ranges is negligible
   (``f64``'s 52-bit mantissa is ~15–16 significant digits; rotary axes
   additionally modulo-wrap, capping magnitude). The core depends only
   on ``libm``. Exactness lives where it belongs — integer increments at
   the boundary — and the ``rust_decimal`` option is closed: it is a
   financial-domain tool (base-10 quantities, legally-required
   exactness, no ``sqrt``), not a motion tool. This matches every
   shipping motion system (TwinCAT NC, CODESYS SoftMotion, CNC cores all
   use ``LREAL``/``double`` for trajectory generation and integer
   increments at the drive).
