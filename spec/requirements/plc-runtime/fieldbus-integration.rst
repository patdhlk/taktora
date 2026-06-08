Fieldbus integration interface
==============================

Gap capability: the shape by which fieldbus protocol stacks (EtherCAT,
Modbus, Profinet, CIP) plug into the runtime — without committing to any
specific protocol implementation in the core.

.. feat:: Fieldbus integration interface
   :id: FEAT_0023
   :status: open
   :satisfies: FEAT_0010

   The shape by which fieldbus protocol stacks (EtherCAT, Modbus, Profinet,
   CIP) plug into the runtime — without committing to any specific
   protocol implementation in the core.

.. req:: Adapter-driven I/O
   :id: REQ_0120
   :status: open
   :satisfies: FEAT_0023

   The runtime shall expose an adapter trait by which a fieldbus driver
   produces ``Channel<T>`` / ``Subscriber<T>`` bindings for ingested
   process variables and consumes ``Publisher<T>`` for outputs.

.. req:: Out-of-tree driver crates
   :id: REQ_0121
   :status: open
   :satisfies: FEAT_0023

   Fieldbus driver implementations shall live in separate crates and shall
   not require modifications to the executor core.

.. req:: Protocol-neutral runtime
   :id: REQ_0122
   :status: open
   :satisfies: FEAT_0010

   The executor core shall not embed any specific fieldbus protocol
   implementation; protocol selection is a deployment concern carried in
   adapter crates.
