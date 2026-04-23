//! Pure-Rust dominant speaker identification for WebRTC applications.
//!
//! This crate implements the three-time-scale subband comparison algorithm
//! described in Volfin & Cohen, "Dominant Speaker Identification for
//! Multipoint Videoconferencing", IEEE 2012. The implementation follows
//! mediasoup's C++ `ActiveSpeakerObserver` for constants and Jitsi's Java
//! `DominantSpeakerIdentification` for the overall structure.
//!
//! Feed it RFC 6464 audio-level observations and it tells you who is talking.
//! No FFI, no WebRTC stack dependency, no unsafe code.
//!
//! # Quick start
//!
//! ```rust
//! use std::time::{Duration, Instant};
//! use dominant_speaker::{ActiveSpeakerDetector, TICK_INTERVAL};
//!
//! let mut detector = ActiveSpeakerDetector::new();
//! let t0 = Instant::now();
//!
//! // Register two participants.
//! detector.add_peer(1, t0);
//! detector.add_peer(2, t0);
//!
//! // Feed audio levels (0 = loud, 127 = silent, per RFC 6464).
//! // Simulate peer 1 speaking for 2 seconds.
//! let mut t = t0;
//! while t < t0 + Duration::from_millis(2000) {
//!     detector.record_level(1, 5, t);   // peer 1: active (low dBov = loud)
//!     detector.record_level(2, 127, t); // peer 2: silent
//!     t += Duration::from_millis(20);
//! }
//!
//! // Call tick() on a timer — returns Some(peer_id) only on speaker change.
//! if let Some(dominant) = detector.tick(t0 + TICK_INTERVAL) {
//!     println!("Dominant speaker: peer {dominant}");
//! }
//! ```
//!
//! See the [README](https://github.com/anatolykoptev/rust-dominant-speaker)
//! for algorithm details, constants reference, and prior art.

#![forbid(unsafe_code)]

mod detector;
mod numerics;
mod speaker;

pub use detector::ActiveSpeakerDetector;

/// Tunable constants for the dominant-speaker election.
///
/// Defaults match mediasoup's production constants exactly.
///
/// # Example
///
/// ```rust
/// use dominant_speaker::{ActiveSpeakerDetector, DetectorConfig};
/// use std::time::Duration;
///
/// // Use defaults (mediasoup-identical behaviour).
/// let default_detector = ActiveSpeakerDetector::new();
///
/// // Raise C1/C2 for a low-bitrate / mobile deployment: fewer speaker switches.
/// let config = DetectorConfig {
///     c1: 5.0,
///     c2: 4.0,
///     tick_interval: Duration::from_millis(500),
///     ..DetectorConfig::default()
/// };
/// let tuned_detector = ActiveSpeakerDetector::with_config(config);
/// ```
#[derive(Debug, Clone)]
pub struct DetectorConfig {
    /// Immediate-window log-ratio threshold (mediasoup: C1).
    pub c1: f64,
    /// Medium-window log-ratio threshold (mediasoup: C2).
    pub c2: f64,
    /// Long-window log-ratio threshold; zero = long window disabled (mediasoup: C3).
    pub c3: f64,
    /// Evaluation cadence. Recommend 300 ms.
    pub tick_interval: std::time::Duration,
    /// Immediate-window subband count (mediasoup: N1).
    ///
    /// The subband width is derived automatically via `ceil(128 / n1)`.
    /// The default of 13 gives a subband width of 10, matching mediasoup.
    pub n1: u8,
    /// Medium-window subband count (mediasoup: N2).
    pub n2: u8,
    /// Long-window subband count (mediasoup: N3).
    pub n3: u8,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            c1: C1,
            c2: C2,
            c3: C3,
            tick_interval: TICK_INTERVAL,
            n1: N1 as u8,
            n2: N2 as u8,
            n3: N3 as u8,
        }
    }
}

// Algorithm constants — ported verbatim from mediasoup's ActiveSpeakerObserver.
// `pub(crate)` so sibling modules can share them without exposing to users.

/// Immediate time-scale log-ratio threshold (mediasoup: C1).
pub(crate) const C1: f64 = 3.0;
/// Medium time-scale log-ratio threshold (mediasoup: C2).
pub(crate) const C2: f64 = 2.0;
/// Long time-scale log-ratio threshold; zero = long window disabled (mediasoup: C3).
pub(crate) const C3: f64 = 0.0;
/// Immediate subband count (mediasoup: N1).
pub(crate) const N1: u32 = 13;
/// Medium subband count (mediasoup: N2).
pub(crate) const N2: u32 = 5;
/// Long subband count (mediasoup: N3).
pub(crate) const N3: u32 = 10;
/// Milliseconds before a stale level entry is replaced with silence (mediasoup: LevelIdleTimeout).
pub(crate) const LEVEL_IDLE_TIMEOUT_MS: u64 = 40;
/// Milliseconds before an idle non-dominant speaker is paused (mediasoup: SpeakerIdleTimeout).
pub(crate) const SPEAKER_IDLE_TIMEOUT_MS: u64 = 60 * 60 * 1000;
/// Long-window threshold used when computing `longs` from `mediums` (mediasoup: LongThreashold).
pub(crate) const LONG_THRESHOLD: u8 = 4;
/// Maximum RFC 6464 audio-level value (mediasoup: MaxLevel).
pub(crate) const MAX_LEVEL: u8 = 127;
/// Minimum RFC 6464 audio-level value (mediasoup: MinLevel).
pub(crate) const MIN_LEVEL: u8 = 0;
/// Window length for adaptive minimum-level estimation (mediasoup: MinLevelWindowLen = 15*1000/20).
pub(crate) const MIN_LEVEL_WINDOW_LEN: u32 = 750;
/// Threshold for medium-window immediate-to-medium downsampling (mediasoup: MediumThreshold).
pub(crate) const MEDIUM_THRESHOLD: u8 = 7;
/// Immediate-buffer length: covers 1 second at 20ms cadence × 5 subbands (mediasoup: ImmediateBuffLen).
pub(crate) const IMMEDIATE_BUFF_LEN: usize = 50;
/// Medium-buffer length (mediasoup: MediumsBuffLen).
pub(crate) const MEDIUMS_BUFF_LEN: usize = 10;
/// Long-buffer length (mediasoup: LongsBuffLen).
pub(crate) const LONGS_BUFF_LEN: usize = 1;
/// Levels ring-buffer length (mediasoup: LevelsBuffLen).
pub(crate) const LEVELS_BUFF_LEN: usize = 50;
/// Floor score; prevents log(0) in ratio computation (mediasoup: MinActivityScore).
pub(crate) const MIN_ACTIVITY_SCORE: f64 = 1.0e-10;

/// Recommended tick interval matching mediasoup's production tuning.
///
/// Call [`ActiveSpeakerDetector::tick`] at this cadence for best results.
pub const TICK_INTERVAL: std::time::Duration = std::time::Duration::from_millis(300);

/// Compute the subband width for a given N1 value.
///
/// Formula: `ceil(128 / n1)`. Mediasoup hard-codes 10 for N1=13 —
/// `ceil(128/13) = 10`. This function generalises it for custom configs.
pub(crate) fn subunit_len_for(n1: u8) -> u8 {
    let n1 = n1.max(1) as u16; // guard against zero
    ((128u16 + n1 - 1) / n1) as u8
}

#[cfg(test)]
mod subunit_tests {
    use super::subunit_len_for;

    #[test]
    fn default_n1_gives_10() {
        assert_eq!(subunit_len_for(13), 10);
    }

    #[test]
    fn n1_10_gives_13() {
        // ceil(128/10) = 13
        assert_eq!(subunit_len_for(10), 13);
    }

    #[test]
    fn n1_8_gives_16() {
        // ceil(128/8) = 16
        assert_eq!(subunit_len_for(8), 16);
    }
}
