# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.1](https://github.com/patdhlk/taktora/compare/taktora-ethercat-esi-v0.2.0...taktora-ethercat-esi-v0.2.1) - 2026-06-01


### Documentation

- *(readme)* Document EtherCAT codegen toolchains, CAN connector, od-core, new examples

## [0.2.0](https://github.com/patdhlk/taktora/compare/taktora-ethercat-esi-v0.1.0...taktora-ethercat-esi-v0.2.0) - 2026-06-01


### Added

- *(ethercat)* ESI device-driver codegen toolchain (FEAT_0050) + real Beckhoff support ([#23](https://github.com/patdhlk/taktora/pull/23))
- *(ethercat-esi)* Parse structured PDOs with padding + alternatives
- *(ethercat-esi)* [**breaking**] Faithful structured IR (PDO/SM/mailbox/DC/OD), located error type

### Changed

- *(ethercat-esi)* Drop dead locals and stale allows in raw_xml capture

### Fixed

- *(ethercat-esi)* Use idiomatic Option<&T> in dictionary_from_profile

### Changed

- **(breaking, DTO behaviour)** Parse real Beckhoff ESI files. `<Name>` is now
  read as a list of localized `<Name LcId=…>` elements, and the device/vendor
  display name is the English (`LcId 1033`) variant when present (else the first
  non-empty name). CDATA-wrapped names decode, entry `<DataType>` elements
  tolerate attributes (e.g. `DScale`), and placeholder `<Sm>` / `<Type>`
  elements with missing attributes no longer reject the document. The public IR
  is unchanged (`name` stays `Option<String>`).

## [0.1.0](https://github.com/patdhlk/taktora/releases/tag/taktora-ethercat-esi-v0.1.0) - 2026-05-31


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
