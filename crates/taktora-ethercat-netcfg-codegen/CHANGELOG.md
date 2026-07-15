# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.10](https://github.com/patdhlk/taktora/compare/taktora-ethercat-netcfg-codegen-v0.1.9...taktora-ethercat-netcfg-codegen-v0.1.10) - 2026-07-15


### Added

- Onboarding golden path + assembly guide (FEAT_0121) ([#186](https://github.com/patdhlk/taktora/pull/186))

## [0.1.9](https://github.com/patdhlk/taktora/compare/taktora-ethercat-netcfg-codegen-v0.1.8...taktora-ethercat-netcfg-codegen-v0.1.9) - 2026-06-27


### Documentation

- *(readme)* Document the J1939 connector and slice channel ([#130](https://github.com/patdhlk/taktora/pull/130))

## [0.1.8](https://github.com/patdhlk/taktora/compare/taktora-ethercat-netcfg-codegen-v0.1.7...taktora-ethercat-netcfg-codegen-v0.1.8) - 2026-06-09


### Added

- *(ethercat)* Netcfg-driven stepper example + op-mode/startup-SDO codegen ([#92](https://github.com/patdhlk/taktora/pull/92))

## [0.1.5](https://github.com/patdhlk/taktora/compare/taktora-ethercat-netcfg-codegen-v0.1.4...taktora-ethercat-netcfg-codegen-v0.1.5) - 2026-06-07


### Fixed

- *(connector-ethercat)* Recover-brick fix + WAGO watchdog drill evidence (REQ_0331/TEST_0863) ([#61](https://github.com/patdhlk/taktora/pull/61))

## [0.1.4](https://github.com/patdhlk/taktora/compare/taktora-ethercat-netcfg-codegen-v0.1.3...taktora-ethercat-netcfg-codegen-v0.1.4) - 2026-06-07


### Added

- *(ethercat)* End-to-end SM-watchdog enforcement — AOU_0016 validated and programmed (REQ_0843–0846) ([#58](https://github.com/patdhlk/taktora/pull/58))

## [0.1.2](https://github.com/patdhlk/taktora/compare/taktora-ethercat-netcfg-codegen-v0.1.1...taktora-ethercat-netcfg-codegen-v0.1.2) - 2026-06-01


### Documentation

- *(readme)* Document EtherCAT codegen toolchains, CAN connector, od-core, new examples

## [0.1.0](https://github.com/patdhlk/taktora/releases/tag/taktora-ethercat-netcfg-codegen-v0.1.0) - 2026-05-31


### Added

- *(examples)* Add ethercat-real-bus (EK1100 + EL1008) ([#8](https://github.com/patdhlk/taktora/pull/8))
- *(examples)* Add ethercat-mock-loop integration example against 0.1.1 ([#2](https://github.com/patdhlk/taktora/pull/2))
- *(channel)* NotifyOutcome surfaces dropped wakeups
- *(channel)* Publisher::loan for true zero-copy sends

### Documentation

- *(readme)* Split Examples section into per-crate and integration subsections
- *(readme)* Mention taktora-log and taktora-log-dlt
- *(spec)* Move specification site to taktora.dev
- *(readme)* Add zenoh connector + SEooC safety concept
- *(readme)* Expand to workspace-wide overview after FEAT_0041
- Workspace README and crate-level rustdoc

### Fixed

- Handle SIGINT/SIGTERM via iceoryx2 WaitSet; drop ctrlc crate

### Doc

- Rewrite README with prominent personal-experiment notice
- How to silence iceoryx2 internal log warnings
