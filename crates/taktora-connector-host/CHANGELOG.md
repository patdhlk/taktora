# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.16](https://github.com/patdhlk/taktora/compare/taktora-connector-host-v0.1.15...taktora-connector-host-v0.1.16) - 2026-06-27


### Documentation

- *(readme)* Document the J1939 connector and slice channel ([#130](https://github.com/patdhlk/taktora/pull/130))

## [0.1.9](https://github.com/patdhlk/taktora/compare/taktora-connector-host-v0.1.8...taktora-connector-host-v0.1.9) - 2026-06-07


### Fixed

- *(connectors)* Health subscriptions are true broadcast streams (REQ_0847, closes #60) ([#63](https://github.com/patdhlk/taktora/pull/63))
- *(connector-ethercat)* Recover-brick fix + WAGO watchdog drill evidence (REQ_0331/TEST_0863) ([#61](https://github.com/patdhlk/taktora/pull/61))

## [0.1.6](https://github.com/patdhlk/taktora/compare/taktora-connector-host-v0.1.5...taktora-connector-host-v0.1.6) - 2026-06-01


### Documentation

- *(readme)* Document EtherCAT codegen toolchains, CAN connector, od-core, new examples

## [0.1.3](https://github.com/patdhlk/taktora/compare/taktora-connector-host-v0.1.2...taktora-connector-host-v0.1.3) - 2026-05-21


### Added

- *(examples)* Add ethercat-real-bus (EK1100 + EL1008) ([#8](https://github.com/patdhlk/taktora/pull/8))

## [0.1.2](https://github.com/patdhlk/taktora/compare/taktora-connector-host-v0.1.1...taktora-connector-host-v0.1.2) - 2026-05-19


### Added

- *(examples)* Add ethercat-mock-loop integration example against 0.1.1 ([#2](https://github.com/patdhlk/taktora/pull/2))

## [0.1.1](https://github.com/patdhlk/taktora/compare/taktora-connector-host-v0.1.0...taktora-connector-host-v0.1.1) - 2026-05-19


### Documentation

- *(readme)* Split Examples section into per-crate and integration subsections
- *(readme)* Mention taktora-log and taktora-log-dlt
- *(spec)* Move specification site to taktora.dev

### Fixed

- *(connector)* Rename ConnectorError::InvalidDescriptor → Configuration
