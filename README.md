# rust-dominant-speaker

[![CI](https://github.com/anatolykoptev/rust-dominant-speaker/actions/workflows/ci.yml/badge.svg)](https://github.com/anatolykoptev/rust-dominant-speaker/actions/workflows/ci.yml)
[![docs.rs](https://docs.rs/rust-dominant-speaker/badge.svg)](https://docs.rs/rust-dominant-speaker)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

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
rust-dominant-speaker = "0.1"
```

```rust
use std::time::{Duration, Instant};
use dominant_speaker::{ActiveSpeakerDetector, TICK_INTERVAL};

let mut detector = ActiveSpeakerDetector::new();
let t0 = Instant::now();

// Register participants.
detector.add_peer(1, t0);
detector.add_peer(2, t0);

// Feed RFC 6464 audio levels: 0 = loudest, 127 = silent.
let mut t = t0;
while t < t0 + Duration::from_millis(2000) {
    detector.record_level(1, 5, t);    // peer 1 is speaking
    detector.record_level(2, 127, t);  // peer 2 is silent
    t += Duration::from_millis(20);
}

// Call tick() on a timer. Returns Some(peer_id) only on speaker change.
if let Some(dominant) = detector.tick(t0 + TICK_INTERVAL) {
    println!("Dominant speaker changed to: peer {dominant}");
}

// Query current speaker without advancing the clock.
assert_eq!(detector.current_dominant(), Some(1));
```

## Algorithm

Three time-scale comparison (Volfin & Cohen, IEEE IEEEI 2012): audio levels are
bucketed into immediate (20ms), medium (200ms), and long (2s) windows. For each
window, a binomial-coefficient activity score is computed. A challenger beats the
incumbent only if their log-ratio exceeds all three thresholds simultaneously,
preventing brief spikes from triggering spurious speaker changes (hysteresis).

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
  [signal-calling-service](https://github.com/signalapp/Signal-Calling-Service)
- **RFC 6464** — input format: audio level in dBov carried in RTP header extensions.

## Extracted from

Originally built as part of [OxPulse Chat](https://oxpulse.chat), published
standalone for the broader Rust WebRTC ecosystem.

## Known limitations

- `peer_id` is `u64`. Generic peer ID support (`PeerId: Eq + Ord + Copy`) is planned for 0.2.
- `std::time::Instant` is used for timing, which does not compile to WASM targets.
  A clock-injection abstraction is planned for 0.2.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
