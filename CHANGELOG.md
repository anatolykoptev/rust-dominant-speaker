# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.1] — 2026-04-22

### Fixed

- **`binomial_coefficient(n, r)` returned 1 instead of 0 when `r > n`.**
  Mathematically C(n, r) = 0 for r > n. The old loop never executed in that case,
  silently returning 1 and producing wrong activity scores under exotic configs.
- **`compute_activity_score` panicked on unsigned underflow when `v_l > n_r`.**
  `(n_r - v_l as u32)` underflowed when a misconfigured `DetectorConfig` (e.g.
  `n2 < 5` or `n3 < 10`) caused `compute_bigs` to produce a bucket count larger
  than the configured window. Debug builds panicked; release builds silently
  wrapped to ~4 billion. Fixed by clamping `v_l` to `n_r` before subtraction.

## [0.2.0] — 2026-04-22

### Breaking

- **`tick()` now returns `Option<SpeakerChange<PeerId>>` instead of `Option<u64>`.**
  Migrate:
  ```rust
  // before
  if let Some(peer_id) = detector.tick(now) { ... }

  // after
  if let Some(change) = detector.tick(now) {
      let peer_id = change.peer_id;
      let confidence = change.c2_margin; // new: 0.0 = bootstrap, >0 = contested
  }
  ```
- **`remove_peer` now takes `&PeerId` instead of `PeerId`.**
  ```rust
  // before: detector.remove_peer(42u64);
  // after:  detector.remove_peer(&42u64);
  ```
- **`current_dominant()` now returns `Option<&PeerId>` instead of `Option<u64>`.**
  Add `copied()` to get the `u64` back: `detector.current_dominant().copied()`.
- `ActiveSpeakerDetector` is now generic: `ActiveSpeakerDetector<PeerId = u64>`.
  The default type parameter preserves backward compatibility for callers that don't
  annotate the type explicitly.

### Added

- **`SpeakerChange<PeerId = u64>`** — richer return from `tick()`.
  `peer_id`: the new dominant speaker. `c2_margin`: medium-window confidence margin.
- **`DefaultDetector`** type alias — `ActiveSpeakerDetector<u64>` for back-compat.
- **`current_top_k(k: usize) -> Vec<PeerId>`** — top K speakers by medium-window score.
- **`peer_scores() -> Vec<(PeerId, f64, f64, f64)>`** — all peers with raw (imm, med, long) scores.
- **`serde` feature flag** — opt-in `Serialize` / `Deserialize` for `DetectorConfig`.
  `tick_interval` serialized as `u64` milliseconds.
- Criterion benchmarks: `tick_5_peers` and `tick_50_peers`.

### Fixed

- **`SUBUNIT_LENGTH_N1` was hard-coded to 10 regardless of `DetectorConfig::n1`.**
  With `n1 != 13`, activity scores were silently wrong. Now derived as `ceil(128 / n1)`.
- **Paused peers no longer win bootstrap elections.** With the `BTreeMap→HashMap`
  migration, paused peers could previously be selected as the bootstrap winner via
  stale scores. The bootstrap path now guards against `sp.paused`.

### Changed

- `ActiveSpeakerDetector` now uses `HashMap` internally (was `BTreeMap`).
  Per-tick determinism of the bootstrap seed is no longer guaranteed — any peer
  can be elected first in an empty room. Real activity overrides the seed within
  one tick in any case. Raw level sum is used as a tiebreaker for equal scores.

## [0.1.1] — 2026-04-22

### Added

- `DetectorConfig` struct — exposes Volfin & Cohen algorithm constants
  (C1/C2/C3, N1/N2/N3, `tick_interval`) with mediasoup production defaults.
- `ActiveSpeakerDetector::with_config(DetectorConfig)` constructor for custom tuning.
- `ActiveSpeakerDetector::config()` accessor returning `&DetectorConfig`.
- Backwards compatible: `ActiveSpeakerDetector::new()` unchanged.

## [0.1.0] — 2026-04-21

### Added

- Initial extraction from OxPulse Chat SFU crate.
- `ActiveSpeakerDetector`: room-level dominant speaker detector with hysteresis.
- Three-time-scale activity scoring (immediate / medium / long windows).
- Adaptive minimum-level tracking for noise-floor estimation.
- Zero dependencies — pure Rust, stdlib only.
- Dual MIT / Apache-2.0 license.
