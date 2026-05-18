# taktora-log

Workspace-wide `log`-crate facade for the [taktora] workspace.

* Re-exports the `log` crate's macros (`info!`/`warn!`/`error!`/`debug!`/`trace!`) and structured `log::kv` types so callers depend on a single crate.
* Defines the `LogSink` trait that backends implement.
* Provides a one-shot `init()` builder that registers the selected `LogSink` as the global `log::Log` exactly once.
* Installs the `tracing-log` bridge so `tracing::*` events flow into the active `LogSink`.
* Ships a console dev fallback used when no daemon is configured and no other logger has been installed.

The default DLT backend lives in [`taktora-log-dlt`]. Integrators may swap it for `log4rs` / `env_logger` / a bespoke logger by calling `log::set_logger` **before** `taktora-log::init()`.

See `spec/requirements/logging.rst` and `spec/architecture/logging.rst` in this repo for the full specification.

[taktora]: https://github.com/patdhlk/taktora
[`taktora-log-dlt`]: https://crates.io/crates/taktora-log-dlt
