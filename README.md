# rust-dominant-speaker

[![CI](https://github.com/anatolykoptev/rust-dominant-speaker/actions/workflows/ci.yml/badge.svg)](https://github.com/anatolykoptev/rust-dominant-speaker/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/rust-dominant-speaker.svg)](https://crates.io/crates/rust-dominant-speaker)
[![docs.rs](https://docs.rs/rust-dominant-speaker/badge.svg)](https://docs.rs/rust-dominant-speaker)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
[![MSRV](https://img.shields.io/badge/MSRV-1.75-blue.svg)](#rust-version)

Pure-Rust library implementing the dominant speaker identification algorithm
used by Jitsi Videobridge and mediasoup. No FFI, no WebRTC stack dependencies —
just feed it RFC 6464 audio-level observations and it tells you who is talking.

## Why this exists

Every Rust SFU currently reimplements this algorithm inline or skips it entirely.
This crate is a faithful, library-shaped extraction so you can drop it in and move on.

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
rust-dominant-speaker = "0.3"
```

```rust
use dominant_speaker::{ActiveSpeakerDetector, SpeakerChange};

let mut detector = ActiveSpeakerDetector::new();

// Timestamps are caller-supplied u64 milliseconds (any epoch).
// In std: let t0 = Instant::now(); let now_ms = t0.elapsed().as_millis() as u64;
// In WASM: performance.now() as u64

// Register participants at t=0.
detector.add_peer(1u64, 0);
detector.add_peer(2u64, 0);

// Feed RFC 6464 audio levels: 0 = loudest, 127 = silent.
let mut t_ms: u64 = 0;
while t_ms < 2000 {
    detector.record_level(1, 5, t_ms);    // peer 1 is speaking
    detector.record_level(2, 127, t_ms);  // peer 2 is silent
    t_ms += 20;
}

// Call tick() on a 300ms timer. Returns Some(SpeakerChange) only on speaker change.
if let Some(change) = detector.tick(300) {
    println!("Dominant speaker: peer {} (confidence: {:.2})", change.peer_id, change.c2_margin);
}

// Query current speaker without advancing the clock.
assert_eq!(detector.current_dominant().copied(), Some(1));
```

## Algorithm

Three time-scale comparison ([Volfin & Cohen, 2012][paper]): audio levels are
bucketed into immediate (20ms), medium (200ms), and long (2s) windows. For each
window, a binomial-coefficient activity score is computed. A challenger beats the
incumbent only if their log-ratio exceeds all three thresholds simultaneously,
preventing brief spikes from triggering spurious speaker changes (hysteresis).

[paper]: https://israelcohen.com/wp-content/uploads/2018/05/IEEEI2012_Volfin.pdf

### Constants (mediasoup production tuning)

| Constant | Value | Meaning |
|----------|-------|---------|
| C1 | 3.0 | Immediate time-scale log-ratio threshold |
| C2 | 2.0 | Medium time-scale log-ratio threshold |
| C3 | 0.0 | Long time-scale threshold (disabled in production) |
| N1 | 13 | Immediate subband count |
| N2 | 5 | Medium subband count |
| N3 | 10 | Long subband count |
| ImmediateBuffLen | 50 | Immediate ring-buffer slots (1s at 20ms cadence × 5 subbands) |
| MediumsBuffLen | 10 | Medium ring-buffer slots |
| LongsBuffLen | 1 | Long ring-buffer slots |
| LevelsBuffLen | 50 | Raw-level ring-buffer slots |
| MinActivityScore | 1e-10 | Score floor; prevents log(0) in ratio |
| LevelIdleTimeout | 40ms | Stale level replaced with silence |
| SpeakerIdleTimeout | 1h | Idle non-dominant peer marked paused |
| TICK_INTERVAL | 300ms | Recommended tick cadence |

## Prior art

- **mediasoup (C++, ISC)** — direct source of constants and algorithm structure.
  [ActiveSpeakerObserver.cpp](https://github.com/versatica/mediasoup/blob/v3/worker/src/RTC/ActiveSpeakerObserver.cpp)
- **Jitsi (Java, Apache-2.0)** — reference algorithm implementation.
  [DominantSpeakerIdentification.java](https://github.com/jitsi/jitsi-utils/blob/master/src/main/java/org/jitsi/utils/dsi/DominantSpeakerIdentification.java)
- **Signal-Calling-Service (Rust, AGPL-3.0)** — a simplified heuristic variant inlined in their backend; not a library.
  [backend/src/audio.rs](https://github.com/signalapp/Signal-Calling-Service/blob/main/backend/src/audio.rs)
- **RFC 6464** — input format: audio level in dBov carried in RTP header extensions.

## Extracted from

Originally built as part of [OxPulse Chat](https://oxpulse.chat), published
standalone for the broader Rust WebRTC ecosystem.

## no_std / WASM

The crate is `#![no_std]` compatible. To use without the standard library:

```toml
[dependencies]
rust-dominant-speaker = { version = "0.3", default-features = false }
```

Timestamps are caller-supplied `u64` milliseconds — no `Instant` dependency:

```rust
// All three methods accept u64 ms in both std and no_std builds.
detector.add_peer(peer_id, now_ms);
detector.record_level(peer_id, level, now_ms);
detector.tick(now_ms);
```

In a browser AudioWorklet, supply `performance.now() as u64` as the timestamp.

Runtime dependencies added for `no_std`: `hashbrown` (hash map) and `libm` (f64 math).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
