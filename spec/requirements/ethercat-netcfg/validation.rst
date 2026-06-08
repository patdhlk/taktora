Validation and bring-up assertions
====================================

Validation requirements for :need:`FEAT_0085` — build-time checks and
generation of bring-up assertions for physical-bus facts.

.. feat:: Validation and bring-up assertions
   :id: FEAT_0085
   :status: open
   :satisfies: FEAT_0080

   Build-time validation of everything derivable from the YAML + ESI,
   plus generation of the bring-up assertions that check facts only the
   physical bus can confirm.

.. req:: Hard build errors for derivable faults
   :id: REQ_0836
   :status: open
   :satisfies: FEAT_0085

   Codegen shall fail the build on: two routings overlapping the same
   bit range in the same SubDevice and direction without
   ``allow_overlap``; a slice extending past the device's declared / ESI
   process-image size; ``bit_length == 0``; a channel referencing a
   non-existent device label; a channel-name or (override-induced)
   configured-address collision; and an ESI contradiction (a device with
   both an ESI reference and disagreeing inline offsets, or an ESI
   hash / revision not matching the lockfile).

.. req:: Warn on unmapped process-image gaps
   :id: REQ_0837
   :status: open
   :satisfies: FEAT_0085

   Codegen shall emit a non-fatal warning for unmapped bit ranges within
   a device's process image. Gaps are legal and often intentional, but
   shall never be silent.

.. req:: Emit bring-up assertions for physical-bus facts
   :id: REQ_0838
   :status: open
   :satisfies: FEAT_0085

   For facts that can only be checked against the physical bus, codegen
   shall emit data driving runtime bring-up assertions: a per-position
   device-identity table (vendor id / product code / revision), the
   declared ``station_alias`` values, and the derived ``expected_wkc``.
   A mismatch at bring-up — wrong device identity or alias at a position,
   or a live working counter diverging from the expectation — shall drive
   the connector's existing ``Degraded`` / ``Down`` health path rather
   than mirroring process data to the wrong terminal.

.. req:: No runtime parsing, no connector-runtime modification
   :id: REQ_0839
   :status: open
   :satisfies: FEAT_0085

   The toolchain shall introduce no runtime YAML parsing and no
   per-instance heap for the bus configuration: all configuration
   resolves at build time into ``&'static`` tables. No change to the
   ``taktora-connector-ethercat`` runtime contracts shall be required to
   consume the generated module.

.. req:: Resolve and emit each output device's SM-watchdog registers
   :id: REQ_0844
   :status: implemented
   :satisfies: FEAT_0085
   :links: BB_0096, TEST_0860

   The ``BusConfig`` shall carry an ``ftti`` (YAML ``ftti_ms``, optional,
   default 100 ms — :need:`AOU_0006`), and each ``DeviceInstance`` shall
   carry an optional per-device ``sm_watchdog_timeout`` override (YAML
   ``sm_watchdog_timeout_ms``). At ``resolve()`` time, for every device
   carrying output (rx) PDOs, the parser shall resolve an effective
   SM-watchdog timeout — the override if present, else FTTI/2 — and
   quantize it to the ESC watchdog registers ``0x0400`` / ``0x0420`` using
   the divider 2498 (a 100 µs tick) and
   ``intervals = ceil(timeout_us / 100)`` clamped to ``1..=u16::MAX``,
   identical to the connector's ``SmWatchdog`` semantics (the arithmetic
   is deliberately duplicated rather than depending on the connector
   crate, per :need:`REQ_0824`). The resolved ``(divider, intervals)``
   shall be exposed on the IR, and codegen shall emit them by chaining
   ``SubDeviceMap::with_sm_watchdog`` on the device's
   ``SubDeviceMap::new`` call. Input-only devices (no rx PDOs) shall
   resolve no watchdog and emit no chain.

   **Rationale.** Safety assumption :need:`AOU_0016` requires every output
   slave's SM watchdog to be enabled with a timeout ≤ FTTI/2 (≤ 50 ms at
   the default 100 ms FTTI), because on a framework-invariant abort the
   master stops emitting process-data frames and the slave watchdog is the
   **sole** mechanism that drives outputs to their safe state
   (:need:`ADR_0065`). The ESC powers up with a 100 ms window — twice the
   bound, bench-verified — and ESI files carry no timeout data, so the
   master must program these registers itself, exactly as IgH
   (``ecrt_slave_config_watchdog``) and TwinCAT (startup register
   download) do. ``network.yaml`` is where both the FTTI budget and any
   per-device override are declared, so it is where the registers are
   resolved.

.. req:: Validate the SM-watchdog bound and enable at config time
   :id: REQ_0845
   :status: implemented
   :satisfies: FEAT_0085
   :links: BB_0096, TEST_0861

   At ``resolve()`` time, for every device carrying output (rx) PDOs, the
   parser shall fail the configuration unless both hold: the QUANTIZED
   effective watchdog window (the ``ceil`` result of :need:`REQ_0844`, not
   the raw request) is ≤ FTTI/2; and the watchdog is enabled. The enable
   check is source-dependent: an ESI-sourced device's output
   process-data sync manager(s) must declare ``watchdog_trigger_enable``
   (control-byte bit 6) true, and an inline-sourced device must attest
   ``sm_watchdog_enabled: true`` (YAML), since an inline source carries no
   ESI control byte to read the enable bit from. Each failure shall be a
   distinct, named error identifying the device label and either the
   offending effective value and the bound, or the missing / disabled
   attestation. Input-only devices shall be untouched by both checks;
   there is no opt-out — FTTI is the only dial.

   **Rationale.** The bound is checked against the quantized value because
   ``ceil`` can push a request that was under the raw FTTI/2 bound over it,
   which would silently miss the safe-state budget (:need:`AOU_0016` /
   :need:`ADR_0065`). The enable check closes the other half of
   :need:`AOU_0016`: a disabled watchdog leaves outputs holding their last
   commanded value indefinitely. ESI control bytes carry the per-SM enable
   bit faithfully (:need:`REQ_0843`), so an ESI source is self-attesting;
   an inline source has no such evidence, so the integrator must attest it
   explicitly or switch to an ESI source.
