# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.6](https://github.com/patdhlk/taktora/compare/taktora-executor-v0.1.5...taktora-executor-v0.1.6) - 2026-06-05


### Added

- *(telemetry)* Off-RT-thread telemetry exporter — seqlock ring + NDJSON + gnuplot (REQ_0111) ([#42](https://github.com/patdhlk/taktora/pull/42))
- *(executor)* Telemetry clock seam + cross-layer contract hardening (FEAT_0021) ([#41](https://github.com/patdhlk/taktora/pull/41))
- *(executor)* FEAT_0021 per-task cycle telemetry — REQ_0107 cross-layer scan index + push/pull stats ([#40](https://github.com/patdhlk/taktora/pull/40))
- *(executor)* Framework internal-fault model — fail-fast panic boundary (FEAT_0024) ([#35](https://github.com/patdhlk/taktora/pull/35))

## [0.1.5](https://github.com/patdhlk/taktora/compare/taktora-executor-v0.1.4...taktora-executor-v0.1.5) - 2026-06-01


### Documentation

- *(readme)* Document EtherCAT codegen toolchains, CAN connector, od-core, new examples

### Fixed

- *(release-plz)* Keep internal dev-deps out of published crates ([#27](https://github.com/patdhlk/taktora/pull/27)) ([#28](https://github.com/patdhlk/taktora/pull/28))

## [0.1.3](https://github.com/patdhlk/taktora/compare/taktora-executor-v0.1.2...taktora-executor-v0.1.3) - 2026-05-21


### Added

- *(examples)* Add ethercat-real-bus (EK1100 + EL1008) ([#8](https://github.com/patdhlk/taktora/pull/8))
- *(executor)* Cycle-overrun fault primitive (FEAT_0018) ([#7](https://github.com/patdhlk/taktora/pull/7))

## [0.1.2](https://github.com/patdhlk/taktora/compare/taktora-executor-v0.1.1...taktora-executor-v0.1.2) - 2026-05-19


### Added

- *(examples)* Add ethercat-mock-loop integration example against 0.1.1 ([#2](https://github.com/patdhlk/taktora/pull/2))

## [0.1.1](https://github.com/patdhlk/taktora/compare/taktora-executor-v0.1.0...taktora-executor-v0.1.1) - 2026-05-19


### Documentation

- *(readme)* Split Examples section into per-crate and integration subsections
- *(readme)* Mention taktora-log and taktora-log-dlt
- *(spec)* Move specification site to taktora.dev
