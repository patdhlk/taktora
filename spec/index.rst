taktora — Architecture & Specification
======================================

Engineering-as-Code specification for `taktora`_ — a Rust workspace that
layers two building blocks on top of `iceoryx2`_:

* ``taktora-executor`` — a high-level execution framework that turns IPC
  events, intervals, and request/response activity into deterministic,
  observable schedules of executable items.
* ``taktora-connector-*`` — a connector framework with typed channels,
  codec-pluggable payloads, uniform connector health, and reference
  EtherCAT and Zenoh connectors exercising the same plugin surface.

The product-facing homepage lives at `taktora.eu`_; this site is the
engineering counterpart and tracks the implementation in detail.

.. warning::

   This specification is part of a personal experiment. APIs, requirements,
   and architecture decisions may shift without notice. See the project
   `README <https://github.com/patdhlk/taktora#readme>`_ for the full caveat.

.. _taktora: https://github.com/patdhlk/taktora
.. _taktora.eu: https://taktora.eu/
.. _iceoryx2: https://github.com/eclipse-iceoryx/iceoryx2

.. toctree::
   :maxdepth: 2
   :caption: Contents

   overview
   requirements/index
   architecture/index
   verification/index
   safety/index

Indices
-------

* :ref:`genindex`
* :ref:`search`
