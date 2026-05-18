# taktora-log-dlt

Pure-Rust AUTOSAR DLT R20-11 backend for [`taktora-log`].

Implements both:

* `taktora_log::LogSink` — for use through the facade crate.
* `log::Log` — for standalone use without the facade, e.g. when an
  integrator wires DLT into their own initialization path.

Talks to a co-located [COVESA dlt-daemon] over a Unix-domain socket
(default) or TCP. No build-time dependency on `libdlt`.

See `spec/requirements/logging.rst` and `spec/architecture/logging.rst`
in the taktora repository for the full specification.

[`taktora-log`]: https://crates.io/crates/taktora-log
[COVESA dlt-daemon]: https://github.com/COVESA/dlt-daemon
