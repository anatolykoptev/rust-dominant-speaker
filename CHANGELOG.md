# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2026-04-21

### Added

- Initial extraction from OxPulse Chat SFU crate.
- `ActiveSpeakerDetector`: room-level dominant speaker detector with hysteresis.
- Three-time-scale activity scoring (immediate / medium / long windows).
- Adaptive minimum-level tracking for noise-floor estimation.
- Zero dependencies — pure Rust, stdlib only.
- Dual MIT / Apache-2.0 license.
