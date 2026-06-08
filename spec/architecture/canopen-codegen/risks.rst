Risks
=====

arc42 §6.

.. risk:: EDS files in the wild are inconsistent
   :id: RISK_0020
   :status: open
   :links: REQ_0725

   Vendor EDS exporters historically produce subtly different
   dialects (LineFeed key variations, comment styles, value
   trimming). Mitigation is the liberal-parser policy
   (:need:`REQ_0725`) — warn and continue rather than reject. The
   parser will accumulate fixture exposure to known-quirky files
   over time; the warning channel makes regressions visible.

.. risk:: serde-ini ecosystem thinness
   :id: RISK_0021
   :status: open
   :links: ADR_0081, REQ_0722

   The Rust serde-INI ecosystem is less mature than serde-XML.
   Both candidate crates (``serde_ini``, ``rust-ini``) have small
   maintainer pools. Mitigation: the façade pattern in
   :need:`ADR_0081` ensures the IR survives a backend swap, and
   the parser surface (``parse(text) -> Result<EdsFile, EdsError>``)
   is small enough that a worst-case fork or replacement is
   low-effort.

.. risk:: CiA 301 OD blow-up on profile-rich devices
   :id: RISK_0022
   :status: open
   :links: REQ_0747, ADR_0075

   CiA 402 servo drives and similar profile-rich devices carry
   200+ OD entries. Generating the full OD table per device would
   balloon the codegen artefact by an order of magnitude.
   Mitigation: OD emission is feature-gated default-off
   (:need:`REQ_0747` mirroring :need:`ADR_0075`); the EtherCAT
   side has already proven this approach for OD-heavy Beckhoff
   modules (cf. :need:`RISK_0010`).

.. risk:: COB-ID base assumptions in generated code
   :id: RISK_0023
   :status: open
   :links: REQ_0753

   Generated code computes ``PdoOut::can_id`` from the current
   ``node_id()`` and the EDS-declared base COB-ID. Devices that
   deviate from CANopen's default COB-ID assignment scheme (e.g.
   manually-overridden bus layouts) would produce wrong CAN IDs.
   Mitigation: the EDS already carries the base COB-ID per PDO
   communication entry; deviations show up as a value other than
   the default. The DCF follow-on (:need:`REQ_0790`) is the right
   place to thread per-bus overrides; this round honestly inherits
   whatever the EDS declares.
