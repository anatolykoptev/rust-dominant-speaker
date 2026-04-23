# Research — Potential Improvements for rust-dominant-speaker

**Last updated:** 2026-04-22 (post v0.3.0)
**Scope:** Forward-looking research for v0.3+. All v0.2 and no_std/WASM v0.3 items are shipped.
**Anchor constraints:**
- Must remain a pure-Rust, zero-dependency crate at its core.
- Must stay sans-I/O — no async, no networking, no WebRTC stack coupling.
- Algorithm correctness relative to mediasoup/Jitsi baseline is a hard constraint —
  behavioral changes require explicit opt-in (config or feature flag).
- Library-shaped work only. Nothing that belongs in the caller's application logic.

---

## What shipped in v0.2.x

All items originally planned for v0.2 are complete:

| Item | Released |
|------|---------|
| Derive `subunit_len` from configured `n1` (`ceil(128/n1)`) | v0.2.0 |
| Generic `ActiveSpeakerDetector<PeerId: Eq+Hash+Clone>` | v0.2.0 |
| `type DefaultDetector = ActiveSpeakerDetector<u64>` alias | v0.2.0 |
| `BTreeMap → HashMap`; `raw_level_sum` tiebreaker for bootstrap elections | v0.2.0 |
| `SpeakerChange<PeerId> { peer_id, c2_margin }` return from `tick()` | v0.2.0 |
| `current_top_k(k) -> Vec<PeerId>` | v0.2.0 |
| `peer_scores() -> Vec<(PeerId, f64, f64, f64)>` | v0.2.0 |
| `serde` feature flag for `DetectorConfig` (tick_interval as u64 ms) | v0.2.0 |
| Criterion benchmarks: `tick_5_peers` (~2.2µs), `tick_50_peers` (~10µs) | v0.2.0 |
| 18 unit tests + 29 adversarial tests (54 total) | v0.2.0 |
| `binomial_coefficient(n,r)` returned 1 when r > n — fixed, early return | v0.2.1 |
| `compute_activity_score` unsigned underflow when v_l > n_r — fixed, clamped | v0.2.1 |

---

## Executive summary for v0.3

1. **`no_std` / WASM unlocks the largest untapped adoption segment.**
   The only `std` dependency is `Instant`. Replace with caller-supplied `u64` milliseconds;
   the crate then compiles to `wasm32-unknown-unknown` and can run in a browser AudioWorklet
   or Insertable Streams worker. No WebRTC competitor offers a WASM-deployable dominant
   speaker implementation. Medium effort (~1 week), high impact.

2. **`ScoringPolicy` trait unlocks the LiveKit and Chime use cases.**
   Both LiveKit (EMA + percentile noise floor) and Amazon Chime (decay score, pluggable
   policy) use simpler algorithms than Volfin/Cohen. A `ScoringPolicy` trait lets callers
   inject their own scorer while reusing our election / top-K / hysteresis machinery.
   Medium effort (3–4 d for trait + EmaPolicy).

3. **proptest / fuzzing for invariant confidence.**
   54 handwritten tests cover known cases. Property-based testing (proptest) can
   verify score monotonicity and election invariants across the full parameter space,
   including edge cases the adversarial suite can't enumerate. The numerics bugs found
   in v0.2.1 are exactly what fuzzing finds cheaply.

4. **Overlap detection opens meeting-quality analytics.**
   When two challengers simultaneously exceed the C2 threshold, we can emit an `Overlap`
   event. No competitor implements this. The timescale buffers we already maintain provide
   80% of the required signal. Define `SpeakerChange` as `#[non_exhaustive]` now to reserve
   the extension point without a breaking change.

---

## Platform support

### `no_std` / embedded / WASM

#### Current state

The crate has zero `[dependencies]` and no I/O. The only `std` usage is
`std::time::Instant` (in every public function taking `now: Instant`) and
`std::collections::HashMap`. `HashMap` requires `std` (not in `alloc`); `Instant`
is not available in `alloc` at all.

#### Proposed approach

Replace `Instant` with `u64` (milliseconds since epoch or call site's epoch). The caller
provides the timestamp — this is already the sans-I/O pattern the crate follows conceptually.

```rust
// Before
pub fn record_level(&mut self, peer_id: PeerId, level_raw: u8, now: Instant);
pub fn tick(&mut self, now: Instant) -> Option<SpeakerChange<PeerId>>;

// After (no_std)
pub fn record_level(&mut self, peer_id: PeerId, level_raw: u8, now_ms: u64);
pub fn tick(&mut self, now_ms: u64) -> Option<SpeakerChange<PeerId>>;
```

Duration arithmetic becomes plain integer subtraction. Monotonicity violations (unlikely,
but possible in WASM environments where `performance.now()` can be throttled) become
`saturating_sub` instead of panics.

For the hash map: `HashMap` is available with the `hashbrown` crate (`no_std`). Feature-gate
it behind `default-features = false, features = ["ahash"]` — or carry `BTreeMap` as the
`no_std` fallback (available in `alloc`).

A `std` feature flag re-exports `Instant`-based convenience wrappers for callers that prefer
the ergonomics of the current API.

**WASM target:** After `no_std`, the crate compiles to `wasm32-unknown-unknown`. In a browser
AudioWorklet, `currentTime * 1000` as `u64` is the timestamp. Client-side dominant speaker
detection eliminates a server round-trip for self-mute UX and local tile highlighting.

#### Tradeoffs

`u64` milliseconds introduces a 1ms quantization (Instant has nanosecond resolution).
The algorithm's minimum meaningful window is 20ms (one audio frame), so 1ms error is
negligible. The current `elapsed_ms / 20` integer division already introduces 20ms
quantization anyway.

**Breaking change.** Semver minor bump (pre-1.0 → v0.3).

#### Recommendations

| # | Item | Effort | Release |
|---|------|--------|---------|
| 1 | Replace `Instant` with `u64` (ms); add `std` feature for Instant-based wrappers | medium (1 wk) | **shipped in v0.3.0** |
| 2 | Add `#![no_std] + extern crate alloc`; use `hashbrown` or `BTreeMap` fallback | small (1 d, after #1) | **shipped in v0.3.0** |
| 3 | Add WASM CI target (`wasm32-unknown-unknown`) | small (0.5 d) | **shipped in v0.3.0** |
| 4 | README note: AudioWorklet deployment example with `performance.now()` timestamp | tiny | **shipped in v0.3.0** |

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
    ActiveLevel     uint8  // RFC 6464 level that counts as "active" (default ~35)
    MinPercentile   uint8  // percentile of the EMA distribution as noise floor (default 25)
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
    fn observe(&mut self, peer_id: &PeerId, volume: u8, now: u64);
    /// Return the current score for `peer_id`. Higher = more likely dominant.
    fn score(&self, peer_id: &PeerId) -> f64;
    /// Called when a peer is removed.
    fn remove(&mut self, peer_id: &PeerId);
}
```

The default impl would be the Volfin/Cohen three-timescale approach. An `EmaPolicy` could
be offered as a second impl. The election logic (hysteresis, top-k) stays in
`ActiveSpeakerDetector`.

**Recommendation:** Add `ScoringPolicy` in v0.3 after the `no_std` / `u64` timestamp
change stabilizes the API. Do not couple it to the initial simplification.

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
detection needs. When `tick()` finds two challengers whose C2 score simultaneously exceeds
`self.config.c2`, we can emit an `Overlap` event alongside the dominant speaker change.

**Recommendation:** Deferred to v0.3+. Mark `SpeakerChange` as `#[non_exhaustive]` now
to reserve the extension point without a future breaking change.

---

## Testing and benchmarking

### Current state (post v0.2.1)

54 tests across four modules:

| Module | Tests | Coverage |
|--------|-------|----------|
| `numerics::tests` | 6 | All math helpers; 4 regression tests for the two v0.2.1 bugs |
| `detector::tests` | 18 | Behavioral invariants: election, hysteresis, idle, top-k, scores |
| `detector::adversarial_tests` | 29 | Edge cases: empty room, single peer, rapid removal, paused peers, large rooms, stress |
| doc-tests | 2 | Public API examples |

Criterion benchmarks: `tick_5_peers` (~2.2µs/iter), `tick_50_peers` (~10µs/iter).

**Remaining gaps:**

- No **property-based tests** verifying score monotonicity across the parameter space
  (proptest or quickcheck). The v0.2.1 bugs were found by adversarial test design, not
  automated invariant search — proptest would have found them faster.
- No **fuzz target** for `record_level` / `tick` with arbitrary byte sequences.
  The public API accepts `u8` level and `u64` peer IDs — low-risk but fuzzing is cheap.
- No **WASM CI target** — blocked on `no_std` work.
- No **benchmark regression CI** — Criterion baselines are not saved in CI.

#### Recommendations

| # | Item | Effort | Release |
|---|------|--------|---------|
| 1 | proptest: score(loud) ≥ score(silent) monotonicity invariant | small (1 d) | v0.3 |
| 2 | proptest: `current_top_k(k).len() ≤ min(k, peer_count)` | small (0.5 d) | v0.3 |
| 3 | cargo-fuzz target for `record_level + tick` | small (1 d) | v0.3 |
| 4 | CI: save Criterion baselines to `gh-pages` branch for regression tracking | small (0.5 d) | v0.3 |

---

## Ecosystem comparison

| Feature | rust-dominant-speaker v0.2.1 | mediasoup (C++) | Jitsi (Java) | LiveKit (Go) | Amazon Chime (TS) |
|---------|------------------------------|-----------------|--------------|--------------|-------------------|
| Algorithm | Volfin & Cohen 2012 | Volfin & Cohen 2012 | Volfin & Cohen 2012 | EMA + percentile | Decay score |
| Hysteresis (3-window) | ✅ | ✅ | ✅ | ❌ | ❌ |
| Generic peer ID | ✅ (`PeerId: Eq+Hash+Clone`) | string | String | string | string |
| Top-K speakers | ✅ `current_top_k` | ❌ | ✅ (recent ring) | ✅ | ✅ |
| Raw score access | ✅ `peer_scores` | ❌ | ❌ | ❌ | partial |
| Confidence margin | ✅ `c2_margin` | ❌ | ❌ | ❌ | partial (score) |
| Serde for config | ✅ (feature flag) | N/A | N/A | N/A | N/A |
| Overlap detection | ❌ | ❌ | ❌ | ❌ | ❌ |
| no_std / WASM | ✅ | ❌ | ❌ | ❌ | ❌ |
| Pluggable scorer | ❌ | ❌ | ❌ | ❌ | ✅ (Policy iface) |
| Criterion benchmarks | ✅ | ❌ | ❌ | ✅ | — |
| Adversarial tests | ✅ (29) | — | — | — | — |
| Zero runtime deps | ✅ | N/A | N/A | N/A | N/A |
| License | MIT/Apache | ISC | Apache-2.0 | Apache-2.0 | Apache-2.0 |

**Reading the table:**
- We are feature-equivalent to mediasoup/Jitsi on the core algorithm. This is the correct baseline.
- We now lead on: generic IDs, top-K, raw scores, confidence margin, serde, adversarial testing.
- WASM/no_std is the next untapped advantage — no competitor offers a WASM-deployable impl.
- Amazon Chime is the only one with a pluggable scorer — worth copying as a v0.3 design.

---

## Prioritized backlog (v0.3+)

| # | Item | Area | Effort | Impact | Release |
|---|------|------|--------|--------|---------|
| 1 | Replace `Instant` with `u64` ms; `std` feature for wrappers | platform | medium (1 wk) | **High** — no_std / WASM unlock | **shipped in v0.3.0** |
| 2 | `#![no_std] + extern crate alloc`; hashbrown or BTreeMap fallback | platform | small (1 d) | High (depends on #1) | **shipped in v0.3.0** |
| 3 | `ScoringPolicy` trait + default Volfin/Cohen impl | architecture | medium (3–4 d) | Medium — extensibility | **v0.3** |
| 4 | `EmaPolicy` as `feature = "ema-scorer"` | algorithm | medium (3–4 d) | Medium — low-latency alt | **v0.3** |
| 5 | proptest: score monotonicity + top-k length invariants | testing | small (1 d) | Medium | **v0.3** |
| 6 | cargo-fuzz target for `record_level + tick` | testing | small (1 d) | Medium | **v0.3** |
| 7 | WASM CI target (`wasm32-unknown-unknown`) | platform | small (0.5 d) | Medium (depends on #2) | **shipped in v0.3.0** |
| 8 | Criterion CI regression tracking (gh-pages baselines) | testing | small (0.5 d) | Low | **v0.3** |
| 9 | `SpeakerChange` as `#[non_exhaustive]`; `Overlap` event variant | algorithm | large (1 wk) | Low today, rising | later |

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
