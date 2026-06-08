Offline ring buffer with reconnect drain
========================================

Keeps the producer hot path non-blocking regardless of daemon state: a
bounded in-memory ring buffers records while the daemon is down and
drains them FIFO on reconnect (a refinement of the DLT backend,
:need:`FEAT_0072`). Overload manifests as the documented drop policy,
never as a producer stall.

.. feat:: Offline ring buffer with reconnect drain
   :id: FEAT_0076
   :status: approved
   :satisfies: FEAT_0072

   When the daemon socket is unavailable (early boot, daemon crash,
   daemon restart), the backend shall buffer records in a bounded
   in-memory ring and drain them on reconnect. The hot path stays
   non-blocking regardless of daemon state; overload manifests as
   the documented drop policy, never as a producer-thread stall.

.. req:: Emission shall not block the calling thread
   :id: REQ_0812
   :status: implemented
   :satisfies: FEAT_0076
   :links: BB_0091, TEST_0812

   Emitting a log record via ``log::*`` macros shall not block the
   calling thread. The producer side of ``taktora-log-dlt`` shall be
   a bounded-queue push only; all socket I/O shall happen on the
   backend's own background flusher thread. The producer shall never
   wait on ``connect``, ``write``, or any other socket syscall.

.. req:: ERROR and FATAL emission shall not heap-allocate
   :id: REQ_0813
   :status: approved
   :satisfies: FEAT_0076

   Emitting an ``ERROR``- or ``FATAL``-level record shall not require
   heap allocation on the producer side. Pre-sized record buffers
   shall be used. ``DEBUG`` / ``TRACE`` / ``INFO`` may format-allocate
   under the formatter's usual rules — only ERROR and FATAL are
   covered by the no-alloc guarantee, on the grounds that those are
   the records that must survive even under memory pressure.

.. req:: Bounded in-memory ring buffers records while daemon is down
   :id: REQ_0814
   :status: implemented
   :satisfies: FEAT_0076
   :links: BB_0091, TEST_0814

   When the daemon socket is unavailable, the DLT backend shall
   buffer records in a bounded in-memory ring (capacity configurable
   at init, with a documented safe default). On reconnect the
   backend shall flush the ring in FIFO order before resuming live
   emission. The ring's storage shall be allocated once at init and
   shall not grow at runtime.

.. req:: Drop-oldest overflow policy with summary record on reconnect
   :id: REQ_0815
   :status: approved
   :satisfies: FEAT_0076

   When the in-memory ring is full and a new record arrives, the
   backend shall drop the oldest record, increment an internal drop
   counter, and continue. The backend shall not panic, shall not
   spin, and shall not block. On the next successful reconnect the
   backend shall emit exactly one summary record at the leading
   position of the drain (DLT App ID = the emitter's, Context ID =
   reserved diagnostic context, message body =
   ``taktora.log.dropped count=N first_dropped_at=...``) and reset
   the counter to zero.
