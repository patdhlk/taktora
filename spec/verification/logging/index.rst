Logging — verification
======================

Test cases verifying the workspace-wide logging facade and its
default DLT backend (see :doc:`../../requirements/logging/index`). Each
``test`` directive ``:verifies:`` one ``req`` parent from
:need:`REQ_0800` .. :need:`REQ_0814` and cites the path and line range
of the covering Rust test under ``crates/taktora-log/tests/`` or
``crates/taktora-log-dlt/tests/``. Mirrors the structure of
:doc:`../bounded-alloc` for diff-friendly review.

The three remaining approved requirements in the chapter
(:need:`REQ_0813`, :need:`REQ_0815`, :need:`REQ_0816`) are deliberately
not promoted here — each one has an open spec-vs-implementation drift
flagged by the audit and needs a regenerate pass before its test can
land at ``status=implemented``.

The test cases are grouped by area (see the toctree): the facade and
backend-swap surface, the DLT backend, runtime log-level control, and
the non-blocking hot path with offline buffering.

.. toctree::
   :maxdepth: 2

   facade-backend-swap
   dlt-backend
   log-level-control
   hot-path-buffering
