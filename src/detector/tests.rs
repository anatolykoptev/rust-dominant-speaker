//! Unit tests for the dominant-speaker detector.

use super::*;
use crate::{DetectorConfig, SpeakerChange};
use std::time::Duration;

fn feed(d: &mut ActiveSpeakerDetector, p: u64, lvl: u8, from: Instant, ms: u64) {
    let mut t = from;
    let end = from + Duration::from_millis(ms);
    while t < end {
        d.record_level(p, lvl, t);
        t += Duration::from_millis(20);
    }
}

/// Helper: assert tick returns a change for the given peer (ignores c2_margin).
fn assert_speaker(change: Option<SpeakerChange>, expected_peer: u64) {
    assert_eq!(
        change.map(|c| c.peer_id),
        Some(expected_peer),
        "expected dominant speaker {expected_peer}"
    );
}

#[test]
fn single_speaker() {
    let mut d = ActiveSpeakerDetector::new();
    let t0 = Instant::now();
    d.add_peer(1, t0);
    assert_speaker(d.tick(t0 + Duration::from_millis(300)), 1);
    assert_eq!(d.tick(t0 + Duration::from_millis(600)), None);
}

#[test]
fn silence_then_speech_switches() {
    let mut d = ActiveSpeakerDetector::new();
    let t0 = Instant::now();
    d.add_peer(1, t0);
    d.add_peer(2, t0);
    feed(&mut d, 1, 5, t0, 2000);
    feed(&mut d, 2, 127, t0, 2000);
    assert_speaker(d.tick(t0 + Duration::from_millis(2050)), 1);
}

#[test]
fn hysteresis_prevents_brief_flap() {
    let mut d = ActiveSpeakerDetector::new();
    let t0 = Instant::now();
    d.add_peer(1, t0);
    d.add_peer(2, t0);
    feed(&mut d, 1, 5, t0, 2000);
    feed(&mut d, 2, 127, t0, 2000);
    assert_speaker(d.tick(t0 + Duration::from_millis(2050)), 1);
    let t1 = t0 + Duration::from_millis(2050);
    feed(&mut d, 1, 127, t1, 400);
    feed(&mut d, 2, 5, t1, 400);
    assert_eq!(d.tick(t1 + Duration::from_millis(450)), None);
}

/// Verify that `with_config` stores the config as-is and `new()` uses the
/// mediasoup defaults (c1=3, c2=2, n1=13).
#[test]
fn detector_with_custom_constants_differs_from_default() {
    let config = DetectorConfig {
        c1: 5.0,
        c2: 4.0,
        c3: 1.0,
        n1: 10,
        n2: 4,
        n3: 8,
        ..DetectorConfig::default()
    };
    let detector: ActiveSpeakerDetector<u64> = ActiveSpeakerDetector::with_config(config.clone());
    assert!((detector.config().c1 - 5.0).abs() < f64::EPSILON);
    assert!((detector.config().c2 - 4.0).abs() < f64::EPSILON);
    assert!((detector.config().c3 - 1.0).abs() < f64::EPSILON);
    assert_eq!(detector.config().n1, 10);
    assert_eq!(detector.config().n2, 4);
    assert_eq!(detector.config().n3, 8);

    let default_detector: ActiveSpeakerDetector<u64> = ActiveSpeakerDetector::new();
    assert!((default_detector.config().c1 - 3.0).abs() < f64::EPSILON);
    assert!((default_detector.config().c2 - 2.0).abs() < f64::EPSILON);
    assert_eq!(default_detector.config().n1, 13);
}

#[test]
fn idle_removal_clears_dominance() {
    let mut d = ActiveSpeakerDetector::new();
    let t0 = Instant::now();
    d.add_peer(1, t0);
    assert_speaker(d.tick(t0 + Duration::from_millis(300)), 1);
    d.remove_peer(&1);
    assert_eq!(d.tick(t0 + Duration::from_millis(600)), None);
}

/// Regression for the SUBUNIT_LENGTH_N1 bug: with n1=10 the subunit width
/// is 13 (not 10), so subband indices stay in 0..9. Without the fix,
/// activity scores were computed against the wrong subband space and
/// elections failed to fire.
#[test]
fn custom_n1_elects_louder_peer() {
    let config = DetectorConfig {
        n1: 10,
        n2: 4,
        n3: 8,
        ..DetectorConfig::default()
    };
    let mut d = ActiveSpeakerDetector::with_config(config);
    let t0 = Instant::now();
    d.add_peer(1, t0);
    d.add_peer(2, t0);
    feed(&mut d, 1, 5, t0, 2000);
    feed(&mut d, 2, 127, t0, 2000);
    assert_speaker(d.tick(t0 + Duration::from_millis(2050)), 1);
}

#[test]
fn speaker_change_has_nonnegative_margin() {
    let mut d = ActiveSpeakerDetector::new();
    let t0 = Instant::now();
    d.add_peer(1, t0);
    d.add_peer(2, t0);
    feed(&mut d, 1, 5, t0, 2000);
    feed(&mut d, 2, 127, t0, 2000);
    let change = d.tick(t0 + Duration::from_millis(2050)).expect("should elect");
    assert_eq!(change.peer_id, 1);
    assert!(change.c2_margin >= 0.0, "margin must be non-negative, got {}", change.c2_margin);
}
