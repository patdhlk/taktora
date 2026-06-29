Risks and technical debt
========================

arc42 §6 — known risks carried by the delivered slice. Each ``risk``
``:links:`` the requirement or decision it bears on.

.. risk:: Unbounded-rejection path unexercised until a second frontend
   :id: RISK_0024
   :status: open
   :links: ADR_0123, REQ_0958

   DBC is bounded by construction, so nothing in the delivered slice ever
   drives the IR's unbounded-sequence rejection. The first frontend that
   can express an unbounded sequence (OMG IDL, ROS 2) will be the first
   real test of that path. Mitigation: ``taktora-idl-core`` already
   models ``Sequence`` with a capacity and ``validate`` already has the
   structural-soundness checks; the rejection logic is in place, only
   untested end-to-end. The follow-on frontend must add the rejection
   tests, not the rejection mechanism.

.. risk:: bool/float CAN fields are rejected, not yet supported
   :id: RISK_0025
   :status: open
   :links: REQ_0956, REQ_0959

   The CAN backend rejects ``bool`` and floating-point signal fields this
   round. The fixture DBC corpus exercised so far does not need them, but
   real vendor databases may. Mitigation: the rejection is explicit (a
   backend error, not silent miscompilation), and adding support touches
   only ``taktora-idl-codegen-can`` and the ``taktora-idl-wire``
   primitives — neither the IR nor the frontend changes.
