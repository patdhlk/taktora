EtherCAT reference connector
============================

A second concrete connector instantiating the framework's contracts. This
cluster ``:satisfies:`` :need:`FEAT_0030`.

.. feat:: EtherCAT reference connector
   :id: FEAT_0041
   :status: open
   :satisfies: FEAT_0030

   A second concrete connector instantiating the framework's contracts:
   ``ethercrab``-backed EtherCAT plugin and gateway with cyclic
   process-data exchange, static per-SubDevice PDO mapping, optional
   Distributed Clocks bring-up, and ``ReconnectPolicy``-driven bus
   re-bringup. The gateway owns a single ethercrab ``MainDevice`` on
   one Linux network interface and runs the TX/RX cycle on a tokio
   sidecar contained inside ``taktora-connector-ethercat``. Linux is the
   only supported host OS in the first cut.

.. req:: EthercatConnector implements Connector
   :id: REQ_0310
   :status: approved
   :satisfies: FEAT_0041

   The connector crate shall expose ``EthercatConnector<C: PayloadCodec>``
   that implements the ``Connector`` trait with
   ``type Routing = EthercatRouting``.

.. req:: EthercatRouting carries SubDevice and PDO addressing
   :id: REQ_0311
   :status: implemented
   :satisfies: FEAT_0041
   :links: BB_0031, TEST_0201

   The ``EthercatRouting`` struct shall identify one process-data slice by
   SubDevice configured address, PDO direction, bit offset within the
   SubDevice's process data, and bit length of the mapped object. It shall
   implement the ``Routing`` marker trait.

.. req:: Single MainDevice per gateway instance
   :id: REQ_0312
   :status: approved
   :satisfies: FEAT_0041

   A single ``EthercatGateway`` instance shall own at most one ethercrab
   ``MainDevice`` bound to one network interface. Multi-NIC deployments
   shall instantiate multiple gateways.

.. req:: Bus reaches OP before serving traffic
   :id: REQ_0313
   :status: approved
   :satisfies: FEAT_0041

   The gateway shall transition the EtherCAT bus to the OP state before
   accepting envelope traffic from the plugin side.

.. req:: Static PDO mapping per SubDevice
   :id: REQ_0314
   :status: approved
   :satisfies: FEAT_0041

   The connector shall accept a static PDO-mapping description per
   SubDevice at build time, declared by the application crate via
   ``EthercatConnectorOptions``.

.. req:: PDO mapping applied during PRE-OP to SAFE-OP transition
   :id: REQ_0315
   :status: implemented
   :satisfies: FEAT_0041
   :links: BB_0033, TEST_0205

   The gateway shall apply the configured PDO mapping by issuing SDO writes
   to the sync-manager assignment indices ``0x1C12`` (RxPDO) and ``0x1C13``
   (TxPDO) during the PRE-OP to SAFE-OP transition.

.. req:: Cycle time configurable with millisecond resolution
   :id: REQ_0316
   :status: implemented
   :satisfies: FEAT_0041
   :links: IMPL_0050, TEST_0206

   The gateway shall accept a configurable cycle duration via
   ``EthercatConnectorOptions::cycle_time`` with a default of 2 ms and a
   minimum resolution of 1 ms.

.. req:: Missed cycle ticks are skipped not queued
   :id: REQ_0317
   :status: implemented
   :satisfies: FEAT_0041
   :links: IMPL_0050, TEST_0207

   When the gateway misses one or more cycle ticks, it shall skip the
   missed ticks rather than queue them for catch-up execution.

.. req:: Distributed Clocks bring-up is opt-in
   :id: REQ_0318
   :status: approved
   :satisfies: FEAT_0041

   The connector shall perform Distributed Clocks bring-up only when
   ``EthercatConnectorOptions::distributed_clocks`` is enabled by the
   application.

.. req:: Working-counter-based health policy
   :id: REQ_0319
   :status: implemented
   :satisfies: FEAT_0041
   :links: IMPL_0050, TEST_0209

   The gateway shall report ``ConnectorHealth::Up`` only when the bus is in
   OP and the working counter on the latest cycle matches the expected
   value derived from the configured PDO mapping.

.. req:: Working-counter mismatch degrades health
   :id: REQ_0320
   :status: approved
   :satisfies: FEAT_0041

   When the working counter on a completed cycle is below the expected
   value, the gateway shall transition ``ConnectorHealth`` to ``Degraded``
   with a reason naming the offending cycle count.

.. req:: Tokio sidecar contained inside the connector crate
   :id: REQ_0321
   :status: approved
   :satisfies: FEAT_0041

   The EtherCAT gateway shall host the ethercrab TX/RX task on a tokio
   runtime contained inside ``taktora-connector-ethercat``. Tokio shall not
   leak into taktora-executor's WaitSet thread.

.. req:: Bridge channels are bounded
   :id: REQ_0322
   :status: approved
   :satisfies: FEAT_0041

   The outbound (taktora-executor → tokio) and inbound (tokio →
   taktora-executor) bridges between the plugin and the gateway sidecar
   shall be bounded channels with configurable capacity in
   ``EthercatConnectorOptions``.

.. req:: Outbound bridge saturation surfaces as BackPressure
   :id: REQ_0323
   :status: approved
   :satisfies: FEAT_0041

   When the outbound bridge channel is full, ``ChannelWriter::send`` shall
   return ``ConnectorError::BackPressure`` and the gateway shall report
   ``ConnectorHealth::Degraded``.

.. req:: Inbound bridge saturation drops PDUs and signals Degraded
   :id: REQ_0324
   :status: implemented
   :satisfies: FEAT_0041
   :links: BB_0034, IMPL_0050, TEST_0214

   When the inbound bridge channel is full, the gateway shall
   (1) increment the per-channel inbound-drop counter exposed via
   ``InboundOutcome::Dropped { count }`` on the bridge's ``try_send``
   return, (2) drop the offending PDU for that cycle, and (3) emit a
   ``ConnectorHealth::Degraded { reason: "dropped N inbound frames" }``
   health transition when the cumulative inbound-drop count crosses
   the connector's configured ``inbound_drop_threshold`` (default 1).
   The Degraded transition is emitted at most once until the
   connector recovers to ``Up`` via the underlying stack's recovery
   path; the cumulative drop count itself is observable through every
   subsequent ``InboundOutcome::Dropped`` return.

.. req:: Linux raw socket required on gateway host
   :id: REQ_0325
   :status: approved
   :satisfies: FEAT_0041

   The gateway shall open the EtherCAT network interface via a Linux raw
   socket, requiring the ``CAP_NET_RAW`` capability on the gateway
   process.

.. req:: Outbound payload written to PDI bit slice per routing
   :id: REQ_0326
   :status: implemented
   :satisfies: FEAT_0041
   :links: IMPL_0050, TEST_0216, TEST_0217, TEST_0218, TEST_0220, TEST_0222

   When a plugin publishes a value through ``ChannelWriter::send``, the
   gateway shall, before the next cycle's ``tx_rx`` call, write the
   codec-encoded payload into the cycle's outbound PDI buffer at the
   bit offset and bit length declared by the channel's
   :need:`REQ_0311` ``EthercatRouting``. The write shall target the
   SubDevice's process image starting at ``bit_offset`` from the
   start of that SubDevice's outputs region, covering exactly
   ``bit_length`` bits. The framework shall preserve adjacent bit
   slices (read-modify-write on partial leading / trailing bytes).

.. req:: Inbound payload read from PDI bit slice per routing
   :id: REQ_0327
   :status: implemented
   :satisfies: FEAT_0041
   :links: IMPL_0050, TEST_0216, TEST_0217, TEST_0221, TEST_0222

   After each cycle's ``tx_rx`` call returns successfully, the gateway
   shall, for every registered inbound channel, extract
   ``bit_length`` bits starting at ``bit_offset`` from the SubDevice's
   process image inputs region (per the channel's
   :need:`REQ_0311` ``EthercatRouting``), and publish the resulting
   byte slice on the channel's inbound iceoryx2 service as a
   ``ConnectorEnvelope`` whose ``payload_len`` is
   ``ceil(bit_length / 8)``. The gateway shall **not** invoke the
   channel's codec on this path — codec decoding is the
   responsibility of the plugin-side ``ChannelReader::try_recv``,
   keeping the gateway a byte-only mover (symmetric with
   :need:`REQ_0326`, where the plugin's ``ChannelWriter::send``
   encodes and the gateway moves the already-encoded bytes). Reads
   shall not modify the PDI buffer.

.. req:: Per-channel routing registry on the gateway
   :id: REQ_0328
   :status: approved
   :satisfies: FEAT_0041

   The gateway shall maintain a registry mapping each open
   ``ChannelDescriptor`` to its ``EthercatRouting`` and direction
   (RxPDO outbound / TxPDO inbound), populated when the application
   calls ``Connector::create_writer`` / ``Connector::create_reader``.
   The cycle loop shall iterate this registry on every cycle —
   draining the outbound bridge for each Rx channel, repopulating
   the inbound iceoryx2 service for each Tx channel — without per-
   cycle heap allocation (no ``Vec`` resize, no ``HashMap``
   re-hash). Required by :need:`REQ_0060` from the steady-state
   posture: connector dispatch shall not allocate.

.. req:: Asymmetric working counter declared per SubDevice
   :id: REQ_0329
   :status: implemented
   :satisfies: FEAT_0041
   :links: IMPL_0050, TEST_0223

   ``SubDeviceMap`` shall carry an explicit ``expected_wkc: u16``
   field. ``BringUp.expected_wkc`` shall be the sum of
   ``SubDeviceMap.expected_wkc`` over the SubDevices that are both
   discovered on the bus and present in ``EthercatConnectorOptions::pdo_map``.
   SubDevices discovered on the bus but absent from ``pdo_map`` shall
   contribute 0.

.. req:: Distributed Clocks cycle path uses tx_rx_dc
   :id: REQ_0330
   :status: open
   :satisfies: FEAT_0041

   When ``EthercatConnectorOptions::distributed_clocks`` is ``true``,
   the cycle shall call ``ethercrab::SubDeviceGroup::tx_rx_dc``;
   otherwise it shall call ``ethercrab::SubDeviceGroup::tx_rx``. This
   refines :need:`REQ_0318` by specifying the per-cycle behaviour of
   the DC opt-in.

   **Implementation status (2026-06-06).** Deferred. ``tx_rx_dc`` is
   only callable when the ``SubDeviceGroup`` typestate is ``HasDc``,
   but the current ``EthercrabBusDriver::bring_up`` walks
   PRE-OP → SAFE-OP → OP via ``into_safe_op`` + ``request_into_op``
   (:need:`REQ_0841`), which still yields ``NoDc``. Honouring this
   requirement requires inserting ``into_pre_op_pdi`` →
   ``configure_dc_sync`` ahead of that walk and threading the
   ``HasDc`` typestate through ``OperationalState`` (and
   ``recover``). The walk already uses ``request_into_op``, so only
   the DC-sync configuration step remains. The mock-side
   ``CycleKind`` recorder (:need:`TEST_0224`) is in place to drive
   that follow-on once it lands.

.. req:: Bus-level recovery on cycle error
   :id: REQ_0331
   :status: implemented
   :satisfies: FEAT_0041
   :links: IMPL_0050, TEST_0225, TEST_0227, TEST_0863

   When ``BusDriver::cycle`` returns ``Err``, the cycle runner shall
   transition health to ``Degraded { reason: "cycle failed: …" }`` and
   consult the configured ``ReconnectPolicy``. For each non-``None``
   backoff returned by the policy the runner shall sleep, then call
   ``BusDriver::recover``. On ``recover`` ``Ok`` the runner shall
   adopt the returned ``BringUp.expected_wkc`` and resume cycling;
   on ``recover`` ``Err`` the runner shall update the Degraded reason
   and continue consulting the policy. On policy exhaustion the
   runner shall transition health to terminal ``Down`` and exit. The
   ``recover`` call shall not consume a new ``PduStorage`` split.
   NIC-level failure (the ``tx_rx_task`` future itself returning
   ``Err``) is terminal and outside this scope.

   A **failed** ``recover`` attempt shall leave the driver
   recoverable: the ``MainDevice`` and ``tx_rx_task`` survive the
   failure so the next policy-driven attempt retries the full walk.
   (Because the ``PduStorage`` split is one-shot, dropping the
   ``MainDevice`` on a failed attempt makes every further recovery
   structurally impossible — observed live on the WAGO 750-354 rig,
   where the first attempt fires while the cable is still unplugged
   and fails with a PDU timeout; a destructive state-take bricked the
   driver into "recover called before bring_up" until process
   restart.)

.. req:: Reconnect policy factory in connector options
   :id: REQ_0332
   :status: implemented
   :satisfies: FEAT_0041
   :links: IMPL_0050, TEST_0225

   ``EthercatConnectorOptions`` shall expose a ``reconnect_policy_factory``
   producing a fresh ``Box<dyn ReconnectPolicy>`` per recovery
   episode. The default factory shall produce
   ``ExponentialBackoff::default()``. The shape and ownership of the
   factory shall mirror ``taktora-connector-can``'s pattern
   (``Arc<dyn Fn() -> Box<dyn ReconnectPolicy> + Send + Sync + 'static>``).

.. req:: Health transitions during recovery
   :id: REQ_0333
   :status: implemented
   :satisfies: FEAT_0041
   :links: IMPL_0050, TEST_0226

   The health state machine shall emit, during a recovery episode,
   exactly the transitions:

   * ``Up → Degraded { reason: "cycle failed: …" }`` on cycle error.
   * ``Degraded → Connecting`` immediately before each ``recover``
     attempt.
   * ``Connecting → Up`` on ``recover`` success.
   * ``Connecting → Degraded { reason: "recover failed: …" }`` on
     ``recover`` error.
   * ``Degraded → Down { reason: "reconnect policy exhausted" }``
     when the policy returns ``None``.

.. req:: SAFE-OP to OP transition exchanges cyclic process data
   :id: REQ_0841
   :status: implemented
   :satisfies: FEAT_0041
   :links: IMPL_0050, TEST_0857

   When transitioning the bus from SAFE-OP to OP — during both
   bring-up and recovery — the gateway shall request the OP state
   without blocking and shall continue cyclic process-data exchange
   until every SubDevice reports OP. While waiting, the gateway shall
   periodically acknowledge latched AL errors (AL Control with the
   error-acknowledge bit plus a renewed OP request) and shall fail
   the attempt after a bounded number of exchanges.

   **Rationale.** SM-watchdog couplers — canonically the WAGO
   750-354, whose ESI declares ``SafeopOpTimeout=100`` ms — refuse OP
   until output process data flows. A blocking state-poll wait
   (ethercrab's ``into_op``) deadlocks against their watchdog, and
   the watchdog error (AL status ``0x001B``) latches once tripped,
   blocking the transition even after traffic resumes, until
   explicitly acknowledged.

.. req:: Bring-up failure is observable via health
   :id: REQ_0842
   :status: implemented
   :satisfies: FEAT_0041
   :links: IMPL_0050, TEST_0858

   When ``BusDriver::bring_up`` fails inside the gateway task spawned
   by ``EthercatConnector::register_with``, the connector shall
   transition health to terminal
   ``Down { reason: "bring-up failed: …" }`` carrying the driver
   error, so the failure is observable through the existing health
   subscription.

   **Rationale.** The spawned task is the only owner of the bring-up
   error; dropping it leaves the connector in ``Connecting``
   indefinitely, indistinguishable from a slow startup. Every
   bring-up defect then presents as a silent hang (missing
   ``CAP_NET_RAW``, SM-watchdog OP refusal, and PDO-mapping faults
   all did, on real hardware) and must be re-diagnosed with local
   instrumentation. The health channel is the connector's
   architecturally designated observability surface (ARCH_0012);
   structured logging remains a separate, future concern.

.. req:: Master programs the SubDevice SM-watchdog registers
   :id: REQ_0846
   :status: implemented
   :satisfies: FEAT_0041
   :links: IMPL_0050, TEST_0862, TEST_0863

   For every ``SubDeviceMap`` carrying ``sm_watchdog: Some(wd)``, the
   gateway shall, during both bring-up and recovery and before the OP
   transition (while the group is in PRE-OP), program the SubDevice's
   watchdog-divider register ``0x0400`` with ``wd.divider`` and its
   SM-watchdog-time register ``0x0420`` with ``wd.intervals``, read
   both registers back, and fail the bring-up / recovery attempt — with
   an error naming the SubDevice address, the register, and the
   expected versus read-back values — on any write error or read-back
   mismatch. A ``SubDeviceMap`` with ``sm_watchdog: None`` shall leave
   the SubDevice's watchdog registers untouched.

   **Rationale.** Safety assumption :need:`AOU_0016` requires an output
   slave's SM watchdog to be enabled with a timeout ≤ FTTI/2 (≤ 50 ms),
   because on a framework-invariant abort the master stops emitting
   process-data frames and the slave watchdog is the **sole** mechanism
   that drives outputs to their safe state (:need:`ADR_0065`). The ESC
   powers up with a 100 ms window — twice the bound — and ESI files
   carry no timeout data, so the master must program these registers
   itself, exactly as IgH (``ecrt_slave_config_watchdog``) and TwinCAT
   (startup register download) do. Silent non-application is this
   domain's signature failure mode — a slave that accepts the write but
   keeps its old window reports healthy while having lost its safe-state
   guarantee — hence the mandatory read-back verify. The failure
   propagates out of bring-up and surfaces as terminal ``Down`` per
   :need:`REQ_0842`.

.. req:: Operator-declared startup SDOs applied before PDO assignment
   :id: REQ_0853
   :status: implemented
   :satisfies: FEAT_0041
   :links: BB_0033, ADR_0103, TEST_0869

   ``SubDeviceMap`` shall carry an optional ``startup_sdos`` list of
   ``StartupSdo`` (index / subindex / value) that the driver writes to the
   matching SubDevice during the PRE-OP transition **before** the PDO
   assignment writes of :need:`REQ_0315`. This lets an application
   configure device parameters (e.g. stepper motor current limits) that
   must be set prior to committing the process-data mapping. An empty list
   produces no writes.
