# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.2](https://github.com/patdhlk/taktora/compare/taktora-connector-zenoh-v0.2.1...taktora-connector-zenoh-v0.2.2) - 2026-05-21


### Added

- *(examples)* Add ethercat-real-bus (EK1100 + EL1008) ([#8](https://github.com/patdhlk/taktora/pull/8))

## [0.2.1](https://github.com/patdhlk/taktora/compare/taktora-connector-zenoh-v0.2.0...taktora-connector-zenoh-v0.2.1) - 2026-05-19


### Added

- *(examples)* Add ethercat-mock-loop integration example against 0.1.1 ([#2](https://github.com/patdhlk/taktora/pull/2))

## [0.2.0](https://github.com/patdhlk/taktora/compare/taktora-connector-zenoh-v0.1.0...taktora-connector-zenoh-v0.2.0) - 2026-05-19


### Documentation

- *(readme)* Split Examples section into per-crate and integration subsections
- *(readme)* Mention taktora-log and taktora-log-dlt
- *(spec)* Move specification site to taktora.dev

### Fixed

- *(connector)* Rename ConnectorError::InvalidDescriptor → Configuration
- *(connector)* Wire InboundBridge into inbound paths and emit Degraded on drops
