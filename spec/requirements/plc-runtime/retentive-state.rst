Retentive state
===============

Gap capability: state that survives process restarts — the equivalent of
NVRAM-backed retentive memory in classical PLCs.

.. feat:: Retentive state
   :id: FEAT_0020
   :status: open
   :satisfies: FEAT_0010

   State that survives process restarts — the equivalent of NVRAM-backed
   retentive memory in classical PLCs.

.. req:: Process-restart persistence
   :id: REQ_0090
   :status: open
   :satisfies: FEAT_0020

   The runtime shall provide a retentive memory abstraction whose declared
   contents persist unchanged across cooperative process restarts.

.. req:: Memory-mapped backing
   :id: REQ_0091
   :status: open
   :satisfies: FEAT_0020

   Retentive memory regions shall be backed by a memory-mapped file with
   a checksum verified at load.

.. req:: Crash-atomic checkpoints
   :id: REQ_0092
   :status: open
   :satisfies: FEAT_0020

   A retentive-memory checkpoint shall be atomic with respect to process
   crash — a concurrent crash shall yield either the pre-checkpoint or
   post-checkpoint contents, never a partial state.

.. req:: Recovery status reporting
   :id: REQ_0093
   :status: open
   :satisfies: FEAT_0020

   At startup, the runtime shall report whether retentive state was loaded
   cleanly, recovered from an incomplete checkpoint (and which version was
   selected), or initialised from defaults because no prior state existed.
