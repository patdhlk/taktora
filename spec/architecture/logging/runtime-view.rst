Runtime view
============

arc42 §6 runtime scenarios for the logging stack. These flows are
derived from the requirement prose and do not introduce new
architectural elements — they make two behaviours concrete: how the
facade selects a backend at one-shot init, and how the offline ring
keeps the producer non-blocking across a daemon outage.

----

One-shot init and backend selection
-----------------------------------

``taktora-log::init()`` runs once per process and picks the active
``log::Log`` by precedence: a pre-installed logger always wins (the
swap path of :need:`REQ_0804`), then an explicitly-built ``LogSink``
such as the DLT backend (:need:`REQ_0803`), and only when neither is
present does the console dev fallback install itself
(:need:`REQ_0816`). A second ``init()`` returns an error rather than
overriding (:need:`REQ_0803`). The bridge from ``tracing`` is installed
on the same init path (:need:`REQ_0805`).

.. mermaid::

   flowchart TD
       Start["taktora-log::init()"]
       AlreadyInit{"global log::Log<br/>already set by init?"}
       PreExisting{"pre-existing log::Log<br/>(integrator-installed)?"}
       Sink{"explicit LogSink<br/>configured on builder?"}
       Bridge["install tracing-log bridge<br/>(LogTracer, once)"]
       UseDlt["register DLT backend<br/>(taktora-log-dlt)"]
       UseConsole["install console<br/>dev fallback (stderr)"]
       KeepExisting["keep pre-existing logger<br/>return PreExistingLogger"]
       Err["return AlreadyInitialized"]

       Start --> AlreadyInit
       AlreadyInit -->|yes| Err
       AlreadyInit -->|no| PreExisting
       PreExisting -->|yes| KeepExisting
       PreExisting -->|no| Sink
       Sink -->|"yes (e.g. DLT)"| UseDlt
       Sink -->|"no daemon, no sink"| UseConsole
       UseDlt --> Bridge
       UseConsole --> Bridge

----

Offline ring buffer and reconnect drain
---------------------------------------

The producer never blocks: ``log::*`` is a bounded-queue push only
(:need:`REQ_0812`), and all socket I/O lives on the background flusher.
While the daemon socket is down the flusher diverts encoded records into
a bounded, pre-allocated ring (:need:`REQ_0814`); when the ring is full
it drops the oldest record and bumps a drop counter (:need:`REQ_0815`).
On reconnect the flusher drains the ring in FIFO order — leading with a
single dropped-count summary record — before resuming live emission
(:need:`REQ_0815`).

.. mermaid::

   sequenceDiagram
       participant P as producer (log::*)
       participant Q as bounded queue
       participant F as background flusher
       participant R as offline ring (bounded)
       participant D as dlt-daemon

       Note over P,Q: hot path never blocks
       P->>Q: push record (no I/O, no wait)
       Q->>F: dequeue

       alt daemon connected
           F->>D: encode + write
       else daemon down
           F->>R: buffer record (FIFO)
           Note over R: ring full → drop oldest,<br/>increment drop counter
       end

       Note over F,D: reconnect (bounded backoff)
       F->>D: summary record<br/>(taktora.log.dropped count=N)
       loop drain in FIFO order
           R-->>F: oldest buffered record
           F->>D: encode + write
       end
       Note over F: resume live emission
