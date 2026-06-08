Console dev fallback
====================

When no DLT daemon socket is configured and no other ``log::Log`` has
been registered, the facade (:need:`FEAT_0071`) installs a
human-readable console formatter so local ``cargo run`` and unit tests
produce visible output. Silent drops are explicitly rejected.

.. feat:: Console dev fallback
   :id: FEAT_0077
   :status: approved
   :satisfies: FEAT_0071

   When no DLT daemon socket is configured and no other ``log::Log``
   implementation has been registered, ``taktora-log::init()`` shall
   install a human-readable console formatter so unit tests and local
   ``cargo run`` produce visible output. Silent drops are explicitly
   rejected.

.. req:: Console fallback installed when no daemon and no other logger
   :id: REQ_0816
   :status: approved
   :satisfies: FEAT_0077

   ``taktora-log::init()`` shall detect the absence of (a) a configured
   DLT daemon socket and (b) any pre-existing ``log::Log``
   implementation, and shall install a console-formatted fallback
   backend in that case. The fallback shall print one line per record
   to ``stderr`` with level, timestamp, target, formatted message,
   and structured key-value pairs. The fallback shall be replaceable
   by any of the other init paths — the fallback is the no-config
   default, not a forced default.
