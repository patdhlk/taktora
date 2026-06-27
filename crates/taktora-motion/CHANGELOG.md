# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.4](https://github.com/patdhlk/taktora/compare/taktora-motion-v0.1.3...taktora-motion-v0.1.4) - 2026-06-27


### Documentation

- *(readme)* Document the J1939 connector and slice channel ([#130](https://github.com/patdhlk/taktora/pull/130))

## [0.1.1](https://github.com/patdhlk/taktora/compare/taktora-motion-v0.1.0...taktora-motion-v0.1.1) - 2026-06-07


### Fixed

- *(connector-ethercat)* Recover-brick fix + WAGO watchdog drill evidence (REQ_0331/TEST_0863) ([#61](https://github.com/patdhlk/taktora/pull/61))

## [0.1.0](https://github.com/patdhlk/taktora/releases/tag/taktora-motion-v0.1.0) - 2026-06-05


### Added

- *(motion)* Motion NC Spine — cyclic NC task over CiA 402 CSP drives (Phases 0–4) ([#31](https://github.com/patdhlk/taktora/pull/31))
- *(examples)* Add ethercat-real-bus (EK1100 + EL1008) ([#8](https://github.com/patdhlk/taktora/pull/8))
- *(examples)* Add ethercat-mock-loop integration example against 0.1.1 ([#2](https://github.com/patdhlk/taktora/pull/2))
- *(channel)* NotifyOutcome surfaces dropped wakeups
- *(channel)* Publisher::loan for true zero-copy sends

### Documentation

- *(readme)* Document EtherCAT codegen toolchains, CAN connector, od-core, new examples
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
