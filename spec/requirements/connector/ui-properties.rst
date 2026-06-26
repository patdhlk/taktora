ViewModel property transport
============================

The observable-state half of the UI connector: how a ViewModel is published,
how its fields are typed, and how the deterministic core is protected from the
UI consumer. This cluster ``:satisfies:`` :need:`FEAT_0092`.

.. feat:: ViewModel property transport
   :id: FEAT_0093
   :status: open
   :satisfies: FEAT_0092

   A ViewModel is a fixed-layout POD struct published atomically over one
   iceoryx2 service with latest-value (history-depth-1) semantics. The
   producer writes the struct into a seqlock cell on the hot path (no
   allocation, torn-read-safe); a non-RT publisher pump snapshots changed
   ViewModels at a configurable cadence, JSON-encodes off-RT, and publishes,
   coalescing intermediate values. Clients reconstruct per-field
   ``PropertyChanged`` by diffing against their last copy.

.. req:: UiConnector implements Connector
   :id: REQ_0855
   :status: draft
   :satisfies: FEAT_0093

   The connector crate shall expose ``UiConnector<C: PayloadCodec>`` that
   implements the ``Connector`` trait with ``type Routing = UiRouting`` and
   ``type Codec = C``, with ``JsonCodec`` as the v1 default codec
   (re-affirming :need:`REQ_0211` / :need:`REQ_0212`). The MVVM primitives
   shall be an additive ergonomics layer over ``create_writer`` /
   ``create_reader``; the shared ``Connector`` trait shall not be modified.

.. req:: ViewModel published as one struct-per-service with latest-value
   :id: REQ_0856
   :status: draft
   :satisfies: FEAT_0093

   Each ViewModel shall be published as a single iceoryx2 pub/sub service
   carrying one fixed-layout struct, published atomically (one envelope per
   update) with publisher history depth 1 so that a late-joining subscriber
   immediately receives the current value. Cross-property coherence shall hold
   within a ViewModel (all fields of one publish share one ``sequence_number``
   / ``timestamp_ns``) but is not guaranteed across ViewModels.

.. req:: UiRouting carries ViewModel/command name and kind
   :id: REQ_0857
   :status: draft
   :satisfies: FEAT_0093

   The ``UiRouting`` struct shall carry the logical name and a ``kind``
   discriminant — ``Property`` | ``Command`` | ``CanExecute`` — with an
   ``Event`` variant reserved for a future lossless event-stream primitive
   (deferred per :need:`FEAT_0092`). It shall implement the ``Routing``
   marker trait.

.. req:: ViewModel fields restricted to a closed POD type set
   :id: REQ_0858
   :status: draft
   :satisfies: FEAT_0093

   ViewModel and command-payload fields shall be drawn from a closed,
   fixed-size POD type set: ``bool``; the integer types ``i8``..``i64`` and
   ``u8``..``u64``; ``f32`` / ``f64``; fixed-length arrays ``[T; K]`` of
   those; inline bounded UTF-8 strings (a ``len: u16`` plus ``[u8; CAP]``);
   nested POD structs of the same; and C-like enums (named discriminants,
   fixed width). Heap-backed types (``Vec``, ``String``, maps),
   data-carrying tagged unions, and 128-bit integers shall be rejected at
   compile time. Same-host operation permits native endianness and alignment.

.. req:: Derive macro computes the envelope payload size
   :id: REQ_0859
   :status: draft
   :satisfies: FEAT_0093

   The ``#[derive(ViewModel)]`` macro shall compute each ViewModel's
   compile-time maximum encoded size and instantiate the channel's
   ``ConnectorEnvelope<N>`` accordingly, so that the const generic ``N`` is
   derived from the declared layout rather than maintained by hand
   (re-affirming :need:`REQ_0201` / :need:`REQ_0205`).

.. req:: Producer writes a seqlock latest-value cell, RT-safe
   :id: REQ_0860
   :status: draft
   :satisfies: FEAT_0093

   Updating a ViewModel from the producing task shall write the POD struct
   into a per-ViewModel seqlock latest-value cell with no heap allocation, no
   iceoryx2 call, and no codec invocation on that path, so the update is safe
   to perform from the executor's RT context. Concurrent reads by the
   publisher pump shall be torn-read-safe (seqlock retry), mirroring the
   ``taktora-telemetry-export`` ring discipline (:need:`FEAT_0038`).

.. req:: Non-RT publisher pump encodes and publishes at a configurable cadence
   :id: REQ_0861
   :status: draft
   :satisfies: FEAT_0093

   A non-RT publisher pump shall snapshot changed ViewModel cells, JSON-encode
   them off the RT thread, and publish them at a configurable UI cadence
   (default in the 30–60 Hz range). The producer (RT) cadence and the publish
   (UI) cadence shall be fully decoupled; intermediate values between two
   publishes shall be coalesced to the latest value (lossy by design for
   state properties). The pump shall not run on the executor's WaitSet
   thread (mirrors :need:`REQ_0321` / :need:`REQ_0258`).

.. req:: Publisher pump skips zero-subscriber ViewModels
   :id: REQ_0862
   :status: draft
   :satisfies: FEAT_0093

   When iceoryx2 reports zero subscribers for a ViewModel's service, the pump
   shall skip encoding and publishing that ViewModel to avoid wasted work, and
   resume on the next tick once a subscriber attaches. The manifest service
   (:need:`REQ_0872`) and the ``SystemViewModel`` heartbeat
   (:need:`REQ_0879`) shall be exempt so a UI can always attach and detect
   liveness.

.. req:: Hot-scalar opt-out promotes a field to its own service
   :id: REQ_0863
   :status: draft
   :satisfies: FEAT_0093

   The author shall be able to opt a single hot scalar field out of its
   enclosing ViewModel struct and publish it on its own iceoryx2 service, for
   cases where one value updates far more often than the rest of the
   ViewModel. The promoted field shall appear in the manifest as its own
   entry; default behaviour remains struct-per-ViewModel.

.. req:: Client reconstructs per-field PropertyChanged by diffing
   :id: REQ_0864
   :status: draft
   :satisfies: FEAT_0093

   The reference client shall raise per-field ``PropertyChanged``-style
   notifications by comparing each received ViewModel struct against the
   previously held copy and signalling only the fields whose values changed
   (net change after coalescing). No per-field change metadata shall be
   carried on the wire in v1.
