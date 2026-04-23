# Research — Potential Improvements for rust-dominant-speaker

**Date:** 2026-04-22
**Scope:** `rust-dominant-speaker` v0.1.1 — grounded in academic + industry research,
read-only research pass.
**Anchor constraints:**
- Must remain a pure-Rust, zero-dependency crate at its core.
- Must stay sans-I/O — no async, no networking, no WebRTC stack coupling.
- Algorithm correctness relative to mediasoup/Jitsi baseline is a hard constraint —
  behavioral changes require explicit opt-in (config or feature flag).
- Library-shaped work only. Nothing that belongs in the caller's application logic.

---

## Executive summary

1. **Most urgent correctness fix: `SUBUNIT_LENGTH_N1` is not derived from `n1` in `DetectorConfig`.**
   When a caller passes `n1 != 13`, `compute_immediates` still divides by the hardcoded
   constant `10`, producing subband indices that exceed the configured `n1` bucket count.
   The result is silently wrong activity scores for custom configs. Small fix (2–3 h),
   critical for anyone using `DetectorConfig` with non-default `n1`. **Do this first.**

2. **Generic peer ID unlocks the largest adoption segment.**
   `u64` is a sensible default but forces callers to maintain a `String → u64` mapping table.
   Making `ActiveSpeakerDetector<PeerId>` generic over `PeerId: Hash + Eq + Clone` is
   backward-compatible via a type alias `type DefaultDetector = ActiveSpeakerDetector<u64>`.
   Medium effort (2–3 days), high impact for real-world integrations.

3. **Add `current_top_k` and raw score access for UI-tier consumers.**
   Every video grid UI needs to rank speakers for tile highlighting, not just know who's
   dominant. `current_top_k(k) -> Vec<PeerId>` is cheap (sort N scores at tick time, N ≤ 50
   in any realistic room). This is the single most-requested pattern across LiveKit, Jitsi,
   and Chime implementations. Small (1 day).

4. **Add a confidence-margin field to the change event.**
   `tick()` returning `Option<PeerId>` loses the C2 margin that decided the election.
   A `SpeakerChange { peer_id, c2_margin: f64 }` return type lets callers implement their
   own smoothing, debounce animations, or raise confidence bars. Small (0.5 day), no
   behavioral change.

5. **`no_std` support opens embedded/WASM targets with minimal cost.**
   The crate has zero runtime dependencies and no I/O. The only blocker is `std::time::Instant`.
   Replace with a caller-supplied monotonic timestamp (`u64` milliseconds) and the crate
   compiles `#![no_std]`. WASM-deployed in a browser AudioWorklet or Insertable Streams
   worker becomes possible. Medium (1 week, due to API surface change).

6. **LiveKit-style EMA approach as an optional alternative scorer.**
   LiveKit uses exponential-decay smoothing (EMA over a percentile window) rather than
   the Volfin/Cohen binomial test. It is simpler, has lower latency (~100ms vs ~300ms),
   and handles burst speakers better. The tradeoff: more false positives in quiet rooms
   and no three-timescale hysteresis. Offer as a `feature = "ema-scorer"` opt-in with a
   `ScoringPolicy` trait. Medium (3–4 days).

---

## Correctness: `n1` / `SUBUNIT_LENGTH_N1` coupling bug

### Current state

`SUBUNIT_LENGTH_N1 = 10` is derived from the mediasoup formula:
`ceil((MAX_LEVEL - MIN_LEVEL + N1) / N1) = ceil((127 + 13) / 13) = 11`
(mediasoup actually hard-codes `10`; we port the constant verbatim).

In `compute_immediates`:
```rust
let imm = lvl / SUBUNIT_LENGTH_N1;  // hardcoded 10
self.immediates[i] = imm;
```

Then in `eval_scores`:
```rust
self.immediate_score = compute_activity_score(self.immediates[0], u32::from(n1), ...);
```

If `n1 = 10` (custom config), `compute_activity_score` treats the score as drawn from
10 subbands, but `imm` values can still be 0–12 (127/10), overflowing the 0–9 range.
`compute_bigs` then counts values `> MEDIUM_THRESHOLD (7)` — it never panics, but the
score is computed against the wrong subband space.

### Fix

Derive `SUBUNIT_LENGTH_N1` from the configured `n1` at construction time:

```rust
fn subunit_len(n1: u8) -> u8 {
    // mediasoup formula: ceil((127 + n1) / n1)
    ((127u16 + n1 as u16 + n1 as u16 - 1) / n1 as u16) as u8
}
```

Store it in `Speaker` (or pass through `eval_scores`). The default `n1 = 13` produces
`subunit_len(13) = 11` (mediasoup hard-codes 10 — keep 10 as the constant for default
to preserve bit-identical behavior; only re-derive for non-default `n1`).

Alternatively: validate in `DetectorConfig` that `n1 == N1` or document the limitation
with a `#[doc(alias = "unsafe")]` note and a runtime assert. The re-derive approach is
cleaner.

### Recommendations

| # | Item | Effort | Release |
|---|------|--------|---------|
| 1 | Derive `subunit_len` from `n1`; store in `Speaker`; fix `compute_immediates` | small (2–3 h) | **v0.2** |
| 2 | Add regression test: `n1=10` config with active speech still elects correctly | small (1 h) | **v0.2** |

---

## API ergonomics

### Generic peer ID

#### Current state

`u64` is hardcoded throughout `ActiveSpeakerDetector`, `Speaker` map key, and return values.
Real WebRTC applications use `String` (Jitsi endpoint IDs), UUID (LiveKit participant IDs),
or custom newtypes. Every caller maintains a `String → u64` interning table — boilerplate
that leaks into application code.

#### Industry survey

| System | Peer ID type |
|--------|-------------|
| mediasoup | `string` (producer ID) |
| Jitsi Videobridge | `String` (endpoint ID) |
| LiveKit | `string` (participant identity) |
| Amazon Chime SDK | `string` (attendee ID) |
| oxpulse-chat | `Uuid` |

All production systems use string IDs. `u64` is a numeric shim that fits databases but
not WebRTC signaling stacks.

#### Proposed API

```rust
pub struct ActiveSpeakerDetector<PeerId = u64>
where
    PeerId: Eq + Hash + Clone,
{
    speakers: HashMap<PeerId, Speaker>,
    current_dominant: Option<PeerId>,
    ...
}

// Backward compat:
pub type DefaultDetector = ActiveSpeakerDetector<u64>;
```

`HashMap` replaces `BTreeMap` for the generic case (hash is O(1) vs BTreeMap's O(log n)).
The determinism property from `BTreeMap` (seed election) can be preserved via a secondary
sorted `Vec<PeerId>` for the bootstrap case only — or drop the determinism guarantee (it
only matters for tests, and we can use `BTreeMap` only in tests via a cfg flag).

#### Recommendations

| # | Item | Effort | Release |
|---|------|--------|---------|
| 1 | Make `ActiveSpeakerDetector<PeerId: Eq + Hash + Clone>` generic | medium (2 d) | v0.2 |
| 2 | Add `type DefaultDetector = ActiveSpeakerDetector<u64>` alias for back-compat | small (0.5 h) | v0.2 |
| 3 | Document the seed-election determinism change in CHANGELOG | small | v0.2 |

### Top-K speakers

#### Current state

`current_dominant()` returns `Option<u64>`. No API for "top 3 speakers" or "all active
speakers ranked by activity". Every grid-layout UI needs this.

#### Industry survey

- **Jitsi Videobridge**: exposes `recent-speakers-count` — an N-deep ring of recent
  dominant speakers for grid tile prioritization.
- **LiveKit**: `ActiveSpeakersUpdate` proto message carries a `Vec<ActiveSpeakerInfo>`
  sorted by audio level, not just the winner.
- **Amazon Chime SDK**: `activeSpeakerDidUpdate(attendees: AttendeeSubscription[])` —
  always passes the full ranked list.

The pattern is universal: callers need a ranked list, not just the winner.

#### Proposed API

```rust
/// Return the top `k` speakers by medium-window activity score, highest first.
///
/// Empty if no peers are registered. Shorter than `k` if fewer peers exist.
/// The result is a snapshot — scores advance only via [`tick`](Self::tick).
pub fn current_top_k(&self, k: usize) -> Vec<PeerId>;

/// Return all peers with their current (immediate, medium, long) scores.
///
/// Useful for dashboards and custom selection logic. Scores are stale
/// between ticks — call after each tick for fresh data.
pub fn peer_scores(&self) -> Vec<(PeerId, f64, f64, f64)>;
```

`current_top_k` sorts `speakers` by `medium_score` descending, takes `k`. The dominant
speaker is always first. Ties broken by `BTreeMap`/`HashMap` iteration order (stable
within a tick, undefined across ticks — document this).

#### Recommendations

| # | Item | Effort | Release |
|---|------|--------|---------|
| 1 | Add `current_top_k(k) -> Vec<PeerId>` | small (1 d) | v0.2 |
| 2 | Add `peer_scores() -> Vec<(PeerId, f64, f64, f64)>` | small (0.5 d) | v0.2 |
| 3 | Tests: top-k order matches expectations after feeding different levels | small (0.5 d) | v0.2 |

### Confidence margin on speaker change

#### Current state

`tick()` returns `Option<u64>`. The C2 log-ratio margin that triggered the switch is
computed internally but discarded. Callers that want to animate a "confidence ring" or
debounce low-confidence switches have no signal.

#### Proposed API

```rust
/// Return value from [`ActiveSpeakerDetector::tick`].
#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerChange<PeerId> {
    /// The new dominant speaker.
    pub peer_id: PeerId,
    /// Medium-window log-ratio margin above the C2 threshold.
    /// Higher = more confident switch. Useful for UI animations.
    pub c2_margin: f64,
}

pub fn tick(&mut self, now: Instant) -> Option<SpeakerChange<PeerId>>;
```

**Breaking change** — but `tick()` is the primary API. Make this v0.2 (minor bump since
we're pre-1.0) or gate behind a feature flag. The `c2_margin` is `best_c2 - self.config.c2`
at the point of the winning challenger — already computed, just not surfaced.

#### Recommendations

| # | Item | Effort | Release |
|---|------|--------|---------|
| 1 | Return `Option<SpeakerChange>` from `tick` with `peer_id + c2_margin` | small (0.5 d) | v0.2 |
| 2 | Add `SpeakerChange` to public re-exports | tiny | v0.2 |

### Serde support

`DetectorConfig` is a natural candidate for JSON/TOML serialization (server config files,
REST API tuning endpoints). Zero runtime cost — feature-gated.

```toml
[features]
serde = ["dep:serde"]

[dependencies]
serde = { version = "1", features = ["derive"], optional = true }
```

```rust
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DetectorConfig { ... }
```

`Duration` requires a custom serde representation (milliseconds as `u64`) — use
`#[serde(with = "serde_duration_ms")]` helper or the `humantime-serde` crate (MIT).

#### Recommendations

| # | Item | Effort | Release |
|---|------|--------|---------|
| 1 | Add `serde` feature flag for `DetectorConfig` | small (0.5 d) | v0.2 |
| 2 | Serialize `tick_interval` as milliseconds (`u64`) | small | v0.2 |

---

## Algorithm alternatives

### Current algorithm: Volfin & Cohen 2012

Three-time-scale binomial activity scoring with mediasoup/Jitsi constants. Election uses
a log-ratio hysteresis: challenger must beat incumbent on all three time scales
simultaneously. Strong hysteresis prevents flapping; latency to elect a new speaker is
~300ms (one tick) after they dominate across the long (2s) window.

**Strengths:** Proven in production (Jitsi 10+ years, mediasoup wide deployment).
Handles multiple simultaneous talkers gracefully. Adaptive min-level tracking compensates
for microphone gain differences across peers.

**Weaknesses:** 300ms minimum latency. Buffer churn for rooms with dozens of peers
changing rapidly. No concept of "confidence" or "probability of speaking".

### LiveKit approach: EMA with percentile noise floor

`pkg/sfu/audio/audiolevel.go` uses exponential moving average over a percentile-based
noise floor:

```go
type AudioLevelConfig struct {
    ActiveLevel    uint8   // RFC 6464 level that counts as "active" (default ~35)
    MinPercentile  uint8   // percentile of the EMA distribution as noise floor (default 25)
    ObserveDuration uint32 // ms of samples to keep
    SmoothIntervals uint32 // EMA decay periods
}
```

`GetLevel()` returns a smoothed float in `[0.0, 1.0]`. The SFU compares smoothed levels
across all participants and picks the highest as dominant. Lower latency (~100ms EMA
window) but more susceptible to noise and offers no hysteresis.

**Applicability:** Simpler to implement and understand. Appropriate for small rooms
(<10 participants) or when switching speed is more important than stability. Would
complement, not replace, the Volfin/Cohen scorer.

### Amazon Chime approach: pluggable scoring policy

Chime's `ActiveSpeakerPolicy` interface:
```typescript
interface ActiveSpeakerPolicy {
    calculateScore(attendeeId: string, volume: number | null, muted: boolean): number;
}
```

`DefaultActiveSpeakerPolicy` uses a decay-based score: each frame's volume contributes
`volume * (1 - decay)`, accumulated, and decays exponentially between frames. The policy
is pluggable — callers inject their own scorer.

This is the most general design. In Rust:

```rust
pub trait ScoringPolicy<PeerId> {
    /// Observe a new audio level for `peer_id`. Called on every `record_level`.
    fn observe(&mut self, peer_id: &PeerId, volume: u8, now: Instant);
    /// Return the current score for `peer_id`. Higher = more likely dominant.
    fn score(&self, peer_id: &PeerId) -> f64;
    /// Called when a peer is removed.
    fn remove(&mut self, peer_id: &PeerId);
}
```

The default impl would be the Volfin/Cohen three-timescale approach. An `EmaPolicy` could
be offered as a second impl. The election logic (hysteresis, top-k) stays in
`ActiveSpeakerDetector`.

**Recommendation:** Add the `ScoringPolicy` trait in v0.3 after the generic PeerId
and top-k APIs stabilize. Do not couple it to the initial API simplification.

#### Recommendations

| # | Item | Effort | Release |
|---|------|--------|---------|
| 1 | `ScoringPolicy` trait definition + default Volfin/Cohen impl | medium (3–4 d) | v0.3 |
| 2 | `EmaPolicy` as `feature = "ema-scorer"` (EMA + percentile noise floor, LiveKit-style) | medium (3–4 d) | v0.3 |
| 3 | Documentation comparing the two policies with guidance on when to use each | small (1 d) | v0.3 |

### Overlap / simultaneous-talker detection

Neither mediasoup, Jitsi, nor the Volfin/Cohen paper handles overlap detection (when two
speakers talk simultaneously). This is an increasingly important signal for:
- Meeting quality metrics ("how often do people interrupt each other?")
- Turn-taking UX cues
- Transcription systems that need to know when to fork speakers

Academic work: Geiger et al., "Online Overlap Detection for Conversational Speech",
INTERSPEECH 2013. Uses energy ratios in overlapping windows — not deep learning, runs
in real-time on a single thread.

**Applicability:** The three-timescale buffer we already maintain is 80% of what overlap
detection needs. When `tick()` finds two challengers whose C2 score exceeds `self.config.c2`
simultaneously, we can emit an `Overlap` event alongside the dominant speaker change.

**Recommendation:** Deferred to v0.3+. Define the event variant now in `SpeakerChange`
as `#[non_exhaustive]` to reserve the extension point.

---

## Platform support

### `no_std` / embedded / WASM

#### Current state

The crate has zero `[dependencies]` and no I/O. The only `std` usage is
`std::time::Instant` (in every public function taking `now: Instant`) and
`std::collections::{BTreeMap, HashMap}`. `BTreeMap` is available in `alloc`; `Instant`
is not.

#### Proposed approach

Replace `Instant` with `u64` (milliseconds since epoch). The caller provides the
timestamp — this is already the sans-I/O pattern the crate follows conceptually.

```rust
// Before
pub fn record_level(&mut self, peer_id: u64, level_raw: u8, now: Instant);
pub fn tick(&mut self, now: Instant) -> Option<SpeakerChange>;

// After (no_std)
pub fn record_level(&mut self, peer_id: PeerId, level_raw: u8, now_ms: u64);
pub fn tick(&mut self, now_ms: u64) -> Option<SpeakerChange<PeerId>>;
```

Duration arithmetic (`elapsed_ms = now - last`) becomes plain subtraction on `u64` —
simpler, faster, no panics on monotonicity violations (saturating_sub instead).

`#![no_std]` + `extern crate alloc` for `BTreeMap`/`Vec`. A `std` feature flag
re-exports the `Instant`-based convenience wrappers for callers that prefer them.

**WASM target:** After `no_std`, the crate compiles to `wasm32-unknown-unknown` and can
run in an AudioWorklet or Insertable Streams worker, enabling client-side dominant speaker
detection without a server round-trip. Low-latency local detection is useful for
self-mute UX and local speaker tile highlighting.

#### Tradeoffs

`u64` milliseconds introduces a 1ms quantization (Instant has nanosecond resolution).
The algorithm's minimum meaningful window is 20ms (one audio frame), so 1ms error is
negligible. The current `elapsed_ms / 20` integer division already introduces 20ms
quantization anyway.

**Breaking change.** Requires a semver bump (v0.2 since we're pre-1.0).

#### Recommendations

| # | Item | Effort | Release |
|---|------|--------|---------|
| 1 | Replace `Instant` with `u64` (ms); add `std` feature for Instant-based wrappers | medium (1 week) | v0.2 or v0.3 |
| 2 | Add `#![no_std] + extern crate alloc` | small (1 d, depends on #1) | same |
| 3 | Add WASM CI target (`wasm32-unknown-unknown`) | small (0.5 d) | same |
| 4 | Add note in README about client-side AudioWorklet deployment | tiny | same |

---

## Testing and benchmarking

### Current test coverage

Four integration tests in `src/detector/tests.rs`:
- `single_speaker` — trivial one-peer case
- `silence_then_speech_switches` — basic election
- `hysteresis_prevents_brief_flap` — hysteresis with 400ms silence
- `detector_with_custom_constants_differs_from_default` — config accessor test (does NOT
  verify election behavior with custom config — only stores and reads values)

Unit tests in `src/numerics.rs`: 2 math smoke tests.

**Notable gaps:**
- No test exercises `n1 != 13` election behavior (the bug above goes untested)
- No test for `remove_peer` clearing dominance mid-election
- No test for the `SPEAKER_IDLE_TIMEOUT_MS` pause path
- No test for rapid clock skew (out-of-order `Instant` calls — guarded by `if now < last`)
- No property-based tests verifying score monotonicity or idempotency

### Benchmark

No benchmarks exist. The hot path (`tick` for a 50-person room) is:
- 50× `eval_scores` → `compute_immediates` (50-iteration loop) + `compute_bigs` twice
- 50× log-ratio comparison (2× `ln()`)

Estimated <1µs per tick for 50 peers (linear scan, no allocation). A Criterion benchmark
would confirm this and detect regressions when the generic PeerId change lands.

#### Recommendations

| # | Item | Effort | Release |
|---|------|--------|---------|
| 1 | Test: `n1=10` config elects correctly (also covers the bug fix above) | small (1 h) | v0.2 |
| 2 | Test: `remove_peer` + subsequent `tick` re-elects correctly | small (0.5 h) | v0.2 |
| 3 | Test: `SPEAKER_IDLE_TIMEOUT_MS` pause path | small (1 h) | v0.2 |
| 4 | Criterion benchmark: 50-peer `tick()` latency | small (1 d) | v0.2 |
| 5 | proptest: score(peer_with_loud_level) ≥ score(peer_with_silent_level) | small (1 d) | v0.3 |

---

## Ecosystem comparison

| Feature | rust-dominant-speaker v0.1.1 | mediasoup (C++) | Jitsi (Java) | LiveKit (Go) | Amazon Chime (TS) |
|---------|------------------------------|-----------------|--------------|--------------|-------------------|
| Algorithm | Volfin & Cohen 2012 | Volfin & Cohen 2012 | Volfin & Cohen 2012 | EMA + percentile | Decay score |
| Hysteresis (3-window) | ✅ | ✅ | ✅ | ❌ | ❌ |
| Dominant speaker only | ✅ | ✅ | ✅ | ✅ | — |
| Top-K speakers | ❌ | ❌ | ✅ (recent ring) | ✅ | ✅ |
| Confidence margin | ❌ | ❌ | ❌ | ❌ | partial (score) |
| Overlap detection | ❌ | ❌ | ❌ | ❌ | ❌ |
| Generic peer ID | ❌ (u64) | string | String | string | string |
| Serde for config | ❌ | N/A | N/A | N/A | N/A |
| no_std / WASM | ❌ | ❌ | ❌ | ❌ | ❌ |
| Pluggable scorer | ❌ | ❌ | ❌ | ❌ | ✅ (Policy iface) |
| Benchmark | ❌ | ❌ | ❌ | ✅ | — |
| Zero dependencies | ✅ | N/A | N/A | N/A | N/A |
| License | MIT/Apache | ISC | Apache-2.0 | Apache-2.0 | Apache-2.0 |

**Reading the table:**
- We are algorithm-identical to mediasoup/Jitsi. This is the correct baseline.
- We're behind on top-K, generic IDs, and serde — all purely API surface work, no algorithm change.
- We're ahead on: zero deps, dual license, Rust memory safety, `forbid(unsafe_code)`.
- WASM/no_std is an untapped advantage — no competitor offers a WASM-deployable implementation.
- Amazon Chime is the only one with a pluggable scorer — worth copying as a v0.3 design.

---

## Prioritized backlog

Ranked by (impact × actionability / effort). v0.2 = next release. v0.3 = one version out.

| # | Item | Area | Effort | Impact | Release |
|---|------|------|--------|--------|---------|
| 1 | Fix `SUBUNIT_LENGTH_N1` derivation from configured `n1` | correctness | tiny (2–3 h) | **High** — silent bug for custom configs | **v0.2** |
| 2 | Add `current_top_k(k) -> Vec<PeerId>` | API | small (1 d) | **High** — universal UI requirement | **v0.2** |
| 3 | Return `SpeakerChange { peer_id, c2_margin }` from `tick` | API | small (0.5 d) | Medium-High — enables confidence UI | **v0.2** |
| 4 | Serde feature flag for `DetectorConfig` | API | small (0.5 d) | Medium — config files / REST tuning | **v0.2** |
| 5 | Tests for `n1` bug, `remove_peer`, idle pause path | testing | small (1 d) | Medium — coverage gaps | **v0.2** |
| 6 | Criterion benchmark for 50-peer `tick()` | testing | small (1 d) | Medium — perf baseline | **v0.2** |
| 7 | `peer_scores() -> Vec<(PeerId, f64, f64, f64)>` | API | small (0.5 d) | Medium — dashboard / debug | **v0.2** |
| 8 | Generic `ActiveSpeakerDetector<PeerId: Eq+Hash+Clone>` | API | medium (2 d) | **High** — adoption by real apps | **v0.2** |
| 9 | Replace `Instant` with `u64` ms; `std` feature for wrappers | platform | medium (1 wk) | High — `no_std` / WASM unlock | v0.3 |
| 10 | `#![no_std] + extern crate alloc` | platform | small (1 d) | High (depends on #9) | v0.3 |
| 11 | WASM CI target | platform | small (0.5 d) | Medium | v0.3 |
| 12 | `ScoringPolicy` trait + default Volfin/Cohen impl | architecture | medium (3–4 d) | Medium — extensibility | v0.3 |
| 13 | `EmaPolicy` as `feature = "ema-scorer"` | algorithm | medium (3–4 d) | Medium — low-latency alt | v0.3 |
| 14 | proptest: score monotonicity invariants | testing | small (1 d) | Medium | v0.3 |
| 15 | Overlap detection (`Overlap` event variant) | algorithm | large (1 wk) | Low today, rising | later |

---

## References

### Academic

- Volfin, I. & Cohen, I., "Dominant Speaker Identification for Multipoint
  Videoconferencing", IEEE ICASSP 2012 / Speech Communication 55 (2013).
  Foundational paper; same algorithm still in production at Jitsi and mediasoup.
- Geiger, J.T., et al., "Online Overlap Detection for Conversational Speech",
  INTERSPEECH 2013. Energy-ratio overlap detection compatible with the timescale buffers
  we already maintain.
- RFC 6464 — "A Real-time Transport Protocol (RTP) Header Extension for
  Client-to-Mixer Audio Level Indication" (Lennox et al., 2011).
  Defines the `0–127 dBov` level format our `record_level` API accepts.

### Reference implementations

- mediasoup — `versatica/mediasoup`, ISC.
  `worker/src/RTC/ActiveSpeakerObserver.cpp` — canonical C++ source for constants.
- Jitsi Videobridge — `jitsi/jitsi-videobridge`, Apache-2.0.
  `jvb/src/main/java/org/jitsi/videobridge/DominantSpeakerIdentification.java`.
- LiveKit — `livekit/livekit`, Apache-2.0.
  `pkg/sfu/audio/audiolevel.go` — EMA + percentile approach.
  `pkg/sfu/audio/audiolevel_test.go` — benchmark reference for test structure.
- Amazon Chime SDK JS — `aws/amazon-chime-sdk-js`, Apache-2.0.
  `src/activespeakerpolicy/DefaultActiveSpeakerPolicy.ts` — decay-based pluggable scorer.
  `src/activespeakerdetector/ActiveSpeakerDetector.ts` — ranked list emission pattern.
- Signal Android — `signalapp/Signal-Android`, AGPL-3.0.
  `AudioLevelMonitor.java` — client-side per-frame polling pattern for self-mute UX.
