# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0](https://github.com/patdhlk/taktora/compare/taktora-connector-ethercat-v0.2.5...taktora-connector-ethercat-v0.3.0) - 2026-06-07


### Added

- *(ethercat)* End-to-end SM-watchdog enforcement — AOU_0016 validated and programmed (REQ_0843–0846) ([#58](https://github.com/patdhlk/taktora/pull/58))

### Fixed

- *(connector-ethercat)* Observable bring-up failures (REQ_0842) + examples version-req sweep ([#55](https://github.com/patdhlk/taktora/pull/55))

## [0.2.5](https://github.com/patdhlk/taktora/compare/taktora-connector-ethercat-v0.2.4...taktora-connector-ethercat-v0.2.5) - 2026-06-06


### Fixed

- *(connector-ethercat)* Cyclic-traffic SAFE-OP→OP walk + WAGO 750-354 example fixes (REQ_0841) ([#53](https://github.com/patdhlk/taktora/pull/53))

## [0.2.2](https://github.com/patdhlk/taktora/compare/taktora-connector-ethercat-v0.2.1...taktora-connector-ethercat-v0.2.2) - 2026-06-01


### Documentation

- *(readme)* Document EtherCAT codegen toolchains, CAN connector, od-core, new examples

## [0.2.0](https://github.com/patdhlk/taktora/compare/taktora-connector-ethercat-v0.1.3...taktora-connector-ethercat-v0.2.0) - 2026-05-29


### Added

- *(ethercat)* Finish the ethercrab bus driver ([#12](https://github.com/patdhlk/taktora/pull/12))

## [0.1.3](https://github.com/patdhlk/taktora/compare/taktora-connector-ethercat-v0.1.2...taktora-connector-ethercat-v0.1.3) - 2026-05-21


### Added

- *(examples)* Add ethercat-real-bus (EK1100 + EL1008) ([#8](https://github.com/patdhlk/taktora/pull/8))

## [0.1.2](https://github.com/patdhlk/taktora/compare/taktora-connector-ethercat-v0.1.1...taktora-connector-ethercat-v0.1.2) - 2026-05-19


### Added

- *(examples)* Add ethercat-mock-loop integration example against 0.1.1 ([#2](https://github.com/patdhlk/taktora/pull/2))

## [0.1.1](https://github.com/patdhlk/taktora/compare/taktora-connector-ethercat-v0.1.0...taktora-connector-ethercat-v0.1.1) - 2026-05-19


### Documentation

- *(readme)* Split Examples section into per-crate and integration subsections
- *(readme)* Mention taktora-log and taktora-log-dlt
- *(spec)* Move specification site to taktora.dev

### Fixed

- *(taktora-connector-ethercat)* Enable tokio time driver in gateway runtime
- *(connector)* Rename ConnectorError::InvalidDescriptor → Configuration
- *(connector)* Wire InboundBridge into inbound paths and emit Degraded on drops
