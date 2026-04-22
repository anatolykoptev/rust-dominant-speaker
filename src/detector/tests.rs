//! Unit tests for the dominant-speaker detector.

use super::*;
use crate::DetectorConfig;
use std::time::Duration;

fn feed(d: &mut ActiveSpeakerDetector, p: u64, lvl: u8, from: Instant, ms: u64) {
    let mut t = from;
    let end = from + Duration::from_millis(ms);
    while t < end {
        d.record_level(p, lvl, t);
        t += Duration::from_millis(20);
    }
}

#[test]
fn single_speaker() {
    let mut d = ActiveSpeakerDetector::new();
    let t0 = Instant::now();
    d.add_peer(1, t0);
    assert_eq!(d.tick(t0 + Duration::from_millis(300)), Some(1));
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
    assert_eq!(d.tick(t0 + Duration::from_millis(2050)), Some(1));
}

#[test]
fn hysteresis_prevents_brief_flap() {
    let mut d = ActiveSpeakerDetector::new();
    let t0 = Instant::now();
    d.add_peer(1, t0);
    d.add_peer(2, t0);
    feed(&mut d, 1, 5, t0, 2000);
    feed(&mut d, 2, 127, t0, 2000);
    assert_eq!(d.tick(t0 + Duration::from_millis(2050)), Some(1));
    let t1 = t0 + Duration::from_millis(2050);
    feed(&mut d, 1, 127, t1, 400);
    feed(&mut d, 2, 5, t1, 400);
    assert_eq!(d.tick(t1 + Duration::from_millis(450)), None);
}

#[test]
fn detector_with_custom_constants_differs_from_default() {
    // Verify that with_config stores the config and the accessor returns it.
    let config = DetectorConfig {
        c1: 5.0,
        c2: 4.0,
        c3: 1.0,
        n1: 10,
        n2: 4,
        n3: 8,
        ..DetectorConfig::default()
    };
    let detector = ActiveSpeakerDetector::with_config(config.clone());
    assert!((detector.config().c1 - 5.0).abs() < f64::EPSILON);
    assert!((detector.config().c2 - 4.0).abs() < f64::EPSILON);
    assert!((detector.config().c3 - 1.0).abs() < f64::EPSILON);
    assert_eq!(detector.config().n1, 10);
    assert_eq!(detector.config().n2, 4);
    assert_eq!(detector.config().n3, 8);

    // Default detector uses mediasoup constants.
    let default_detector = ActiveSpeakerDetector::new();
    assert!((default_detector.config().c1 - 3.0).abs() < f64::EPSILON);
    assert!((default_detector.config().c2 - 2.0).abs() < f64::EPSILON);
    assert_eq!(default_detector.config().n1, 13);
}

#[test]
fn idle_removal_clears_dominance() {
    let mut d = ActiveSpeakerDetector::new();
    let t0 = Instant::now();
    d.add_peer(1, t0);
    assert_eq!(d.tick(t0 + Duration::from_millis(300)), Some(1));
    d.remove_peer(1);
    assert_eq!(d.tick(t0 + Duration::from_millis(600)), None);
}
