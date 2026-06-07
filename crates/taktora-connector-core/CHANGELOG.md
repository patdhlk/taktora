# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.7](https://github.com/patdhlk/taktora/compare/taktora-connector-core-v0.2.6...taktora-connector-core-v0.2.7) - 2026-06-07


### Fixed

- *(connector-ethercat)* Recover-brick fix + WAGO watchdog drill evidence (REQ_0331/TEST_0863) ([#61](https://github.com/patdhlk/taktora/pull/61))

## [0.2.5](https://github.com/patdhlk/taktora/compare/taktora-connector-core-v0.2.4...taktora-connector-core-v0.2.5) - 2026-06-01


### Documentation

- *(readme)* Document EtherCAT codegen toolchains, CAN connector, od-core, new examples

## [0.2.3](https://github.com/patdhlk/taktora/compare/taktora-connector-core-v0.2.2...taktora-connector-core-v0.2.3) - 2026-05-29


### Added

- *(ethercat)* Finish the ethercrab bus driver ([#12](https://github.com/patdhlk/taktora/pull/12))

## [0.2.2](https://github.com/patdhlk/taktora/compare/taktora-connector-core-v0.2.1...taktora-connector-core-v0.2.2) - 2026-05-21


### Added

- *(examples)* Add ethercat-real-bus (EK1100 + EL1008) ([#8](https://github.com/patdhlk/taktora/pull/8))

## [0.2.1](https://github.com/patdhlk/taktora/compare/taktora-connector-core-v0.2.0...taktora-connector-core-v0.2.1) - 2026-05-19


### Added

- *(examples)* Add ethercat-mock-loop integration example against 0.1.1 ([#2](https://github.com/patdhlk/taktora/pull/2))

## [0.2.0](https://github.com/patdhlk/taktora/compare/taktora-connector-core-v0.1.0...taktora-connector-core-v0.2.0) - 2026-05-19


### Documentation

- *(readme)* Split Examples section into per-crate and integration subsections
- *(readme)* Mention taktora-log and taktora-log-dlt
- *(spec)* Move specification site to taktora.dev

### Fixed

- *(connector)* Rename ConnectorError::InvalidDescriptor → Configuration
