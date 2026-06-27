UI command channel
==================

The actuation half of the UI connector: how a View invokes a command, how the
application acknowledges it, and how double-actuation is prevented. This
cluster ``:satisfies:`` :need:`FEAT_0092`.

.. feat:: UI command channel
   :id: FEAT_0094
   :status: open
   :satisfies: FEAT_0092

   A command is an acceptance-ack request-response: the View sends an
   invocation carrying a ``correlation_id``; an off-RT handler validates it,
   enqueues the effect into the executor, and replies ``Accepted`` or
   ``Rejected { code, message }``. Progress and final outcome are observed
   through ViewModel properties, not through a held-open call. ``CanExecute``
   is a published boolean property. Delivery is at-most-once via
   ``correlation_id`` dedupe; commands flagged idempotent may be auto-retried
   by the client.

.. req:: Commands use acceptance-ack request-response
   :id: REQ_0865
   :status: implemented
   :satisfies: FEAT_0094
   :links: BB_0046, TEST_0878, TEST_0881

   A command invocation shall be a request-response exchange keyed by the
   envelope ``correlation_id`` (:need:`REQ_0204`). The application shall reply
   with a bounded, prompt acknowledgement of either ``Accepted`` or
   ``Rejected { code, message }``. The acknowledgement shall convey command
   *acceptance*, not operation *completion*; long-running effects and the
   final outcome shall be observed through ViewModel properties
   (:need:`REQ_0856`). No per-command state shall be held open awaiting
   completion.

.. req:: CanExecute is a published boolean property
   :id: REQ_0866
   :status: implemented
   :satisfies: FEAT_0094
   :links: BB_0046, TEST_0874, TEST_0879

   Each command's ``CanExecute`` state shall be exposed as a published boolean
   property (``kind = CanExecute`` per :need:`REQ_0857`); the
   ``CanExecuteChanged`` event is that property updating. Clients shall gate
   the command's UI affordance on this value.

.. req:: Command delivery is at-most-once via correlation-id dedupe
   :id: REQ_0867
   :status: implemented
   :satisfies: FEAT_0094
   :links: BB_0046, TEST_0878, TEST_0879

   The application shall keep a bounded LRU of recently-seen
   ``correlation_id`` values mapped to their acknowledgement. A retry carrying
   a previously-seen ``correlation_id`` shall replay the cached acknowledgement
   without re-executing the command effect. Clients shall reuse the same
   ``correlation_id`` when retrying within a timeout, so an actuating command
   is delivered at most once.

.. req:: Idempotent commands are flagged for opt-in auto-retry
   :id: REQ_0868
   :status: implemented
   :satisfies: FEAT_0094
   :links: BB_0047, TEST_0874

   A command may be flagged idempotent at authoring time (e.g.
   ``#[command(idempotent)]``); the flag shall be carried in the manifest
   (:need:`REQ_0873`). For an idempotent command the client may auto-retry
   (timeout and max-attempts client-side), including across an epoch change
   (:need:`REQ_0882`). For a non-idempotent command the client shall not
   auto-retry across an epoch change; in-flight non-idempotent commands at an
   epoch boundary shall be surfaced as "outcome unknown" for operator
   resolution rather than re-sent.

.. req:: Rejected carries a closed reason-code set
   :id: REQ_0869
   :status: implemented
   :satisfies: FEAT_0094
   :links: BB_0045, TEST_0879

   A ``Rejected`` acknowledgement shall carry a reason ``code`` from a closed,
   framework-defined set — ``CanExecuteFalse``, ``InvalidArgs``, ``Faulted``,
   ``BackPressure``, ``UnknownCommand``, ``ContractMismatch`` — plus a bounded
   human-readable ``message`` for application-specific detail. The code set is
   part of the contract so clients can react programmatically.

.. req:: Command handler runs off the RT thread
   :id: REQ_0870
   :status: implemented
   :satisfies: FEAT_0094
   :links: BB_0046, TEST_0878

   The command handler that reads invocations, validates them, and replies
   shall run off the executor's RT/WaitSet thread; it shall enqueue the
   command's effect into the executor through the normal bounded path. A
   command shall never execute its effect synchronously on the receiving
   thread (mirrors :need:`REQ_0321` / :need:`REQ_0258`).

.. req:: Command channel is bounded and surfaces BackPressure
   :id: REQ_0871
   :status: implemented
   :satisfies: FEAT_0094
   :links: BB_0046, TEST_0879

   The command request channel shall be bounded with a capacity configurable
   via the connector options. When the channel is full, the connector shall
   acknowledge the invocation ``Rejected { code: BackPressure }`` rather than
   block or drop silently, reusing ``ConnectorError::BackPressure``
   (re-affirming :need:`REQ_0405`'s posture for the command path).
