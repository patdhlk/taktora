Event-driven I/O dispatch
=========================

Foundation capability (taktora-executor v0.1): inter-process inputs and
outputs flow through iceoryx2 channels so producers wake consumers without
polling.

.. feat:: Event-driven I/O dispatch
   :id: FEAT_0012
   :status: implemented
   :satisfies: FEAT_0010

   Inter-process inputs and outputs flow through iceoryx2 channels so
   producers wake consumers without polling.

.. req:: Subscriber-triggered ingestion
   :id: REQ_0010
   :status: implemented
   :satisfies: FEAT_0012
   :links: BB_0026, TEST_0107

   The runtime shall trigger an item's ``execute`` whenever a declared
   ``Subscriber<T>`` receives a new sample.

.. req:: Publisher-driven emission
   :id: REQ_0011
   :status: implemented
   :satisfies: FEAT_0012
   :links: BB_0026, TEST_0108

   The runtime shall expose ``Publisher<T>`` send paths (``send_copy``,
   ``loan_send``, ``loan``) for emitting outputs to other processes.

.. req:: Zero-copy IPC transport
   :id: REQ_0012
   :status: implemented
   :satisfies: FEAT_0012
   :links: BB_0026, TEST_0109

   Pub/sub data transfer between processes shall be zero-copy across
   shared memory via iceoryx2; receivers shall obtain a borrowed view of
   the producer's payload, not a deserialised copy.

.. req:: Notification-drop visibility
   :id: REQ_0013
   :status: implemented
   :satisfies: FEAT_0012
   :links: BB_0026, TEST_0113

   The runtime shall surface dropped event-service notifications to the
   sender as a non-error counter (``NotifyOutcome::listeners_notified``)
   so the sender can detect consumer back-pressure programmatically.
