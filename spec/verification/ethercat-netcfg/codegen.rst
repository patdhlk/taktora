Codegen tests
=============

Test cases for the codegen crate (:need:`FEAT_0082`). Per-crate,
snapshot-based. Live under ``crates/ethercat-netcfg-codegen/tests/``.

.. test:: Generated module emits the expected static PDO_MAP
   :id: TEST_0834
   :status: open
   :verifies: REQ_0825

   Golden-file test: codegen over the WAGO fixture produces a
   ``pub static PDO_MAP: &[SubDeviceMap]`` whose single entry carries the
   computed address, the mapped ``PdoEntry`` slices, and the derived
   ``expected_wkc``. Snapshot compared against a checked-in golden module.

.. test:: Generated routing and channel-name constants match the bindings
   :id: TEST_0835
   :status: open
   :verifies: REQ_0826

   For each channel binding in the fixture, the generated module exposes
   a named ``EthercatRouting`` constant carrying the resolved subdevice
   address, direction, bit offset, and bit length, plus the channel-name
   string constant. Asserted against the golden snapshot.

.. test:: Configured addresses follow bus position, override honoured
   :id: TEST_0836
   :status: open
   :verifies: REQ_0827

   A three-device fixture generates addresses ``0x1000``, ``0x1001``,
   ``0x1002`` in list order. A second fixture with an explicit
   ``address: 0x1005`` override on the middle device generates
   ``0x1000``, ``0x1005``, ``0x1002``. Confirms positional assignment and
   the escape hatch of :need:`ADR_0093`.

.. test:: expected_wkc is derived from PDO directions
   :id: TEST_0837
   :status: open
   :verifies: REQ_0828

   Property test over fixtures: a TxPDO-only device generates
   ``expected_wkc = 2``, an RxPDO-only device ``1``, a both-directions
   device ``3``, and a PDO-less coupler ``0`` — the canonical 0/1/2/3
   rule. No fixture is able to express an override value; the schema
   carries no such field (guards :need:`ADR_0095`).

.. test:: Generated output is byte-deterministic
   :id: TEST_0838
   :status: open
   :verifies: REQ_0829

   Codegen runs twice over the same fixture and pinned ESI inputs; the
   two emitted modules are byte-identical. A variant reorders unrelated
   YAML keys and confirms the output is unchanged. No timestamp or
   hash-map iteration order leaks into the file.
