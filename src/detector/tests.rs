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

#[test]
fn top_k_returns_loudest_first() {
    let mut d = ActiveSpeakerDetector::new();
    let t0 = Instant::now();
    d.add_peer(1, t0);
    d.add_peer(2, t0);
    d.add_peer(3, t0);
    feed(&mut d, 1, 5, t0, 2000);   // loudest
    feed(&mut d, 2, 50, t0, 2000);  // moderate
    feed(&mut d, 3, 127, t0, 2000); // silent
    d.tick(t0 + Duration::from_millis(2050));
    // Second tick to ensure scores are fully settled.
    d.tick(t0 + Duration::from_millis(2350));
    let top2 = d.current_top_k(2);
    assert_eq!(top2.len(), 2);
    assert_eq!(top2[0], 1, "loudest peer should be first");
    assert_eq!(top2[1], 2, "moderate peer should be second");
}

#[test]
fn top_k_larger_than_peers_returns_all() {
    let mut d = ActiveSpeakerDetector::new();
    let t0 = Instant::now();
    d.add_peer(1, t0);
    d.add_peer(2, t0);
    d.tick(t0 + Duration::from_millis(300));
    let top = d.current_top_k(10);
    assert_eq!(top.len(), 2);
}

#[test]
fn peer_scores_returns_all_peers() {
    let mut d = ActiveSpeakerDetector::new();
    let t0 = Instant::now();
    d.add_peer(1, t0);
    d.add_peer(2, t0);
    feed(&mut d, 1, 5, t0, 1000);
    d.tick(t0 + Duration::from_millis(1050));
    let scores = d.peer_scores();
    assert_eq!(scores.len(), 2);
    // All scores must be finite and non-negative.
    for (_, imm, med, lng) in &scores {
        assert!(imm.is_finite() && *imm >= 0.0);
        assert!(med.is_finite() && *med >= 0.0);
        assert!(lng.is_finite() && *lng >= 0.0);
    }
}

#[test]
fn remove_dominant_clears_dominance_for_next_tick() {
    let mut d = ActiveSpeakerDetector::new();
    let t0 = Instant::now();
    d.add_peer(1, t0);
    d.add_peer(2, t0);
    feed(&mut d, 1, 5, t0, 2000);
    feed(&mut d, 2, 127, t0, 2000);
    assert_speaker(d.tick(t0 + Duration::from_millis(2050)), 1);
    assert_eq!(d.current_dominant(), Some(&1));

    // Remove the dominant speaker.
    d.remove_peer(&1);
    assert_eq!(d.current_dominant(), None, "dominance must clear on remove");

    // Next tick with only peer 2 remaining should elect peer 2.
    assert_speaker(d.tick(t0 + Duration::from_millis(2400)), 2);
}

#[test]
fn idle_peer_gets_paused_and_excluded_from_election() {
    // Verify that a paused peer is skipped in the election loop.
    // We set paused manually via the speakers_mut() test accessor.
    let mut d = ActiveSpeakerDetector::new();
    let t0 = Instant::now();
    d.add_peer(1, t0);
    d.add_peer(2, t0);
    // Make peer 2 loud and peer 1 silent, but then manually pause peer 2.
    feed(&mut d, 2, 5, t0, 2000);
    feed(&mut d, 1, 127, t0, 2000);
    if let Some(sp) = d.speakers_mut().get_mut(&2) {
        sp.paused = true;
    }
    // With peer 2 paused, peer 1 is the only eligible candidate and must be elected.
    assert_speaker(d.tick(t0 + Duration::from_millis(2050)), 1);
}

#[test]
fn tick_with_no_peers_returns_none() {
    let mut d: ActiveSpeakerDetector = ActiveSpeakerDetector::new();
    let t0 = Instant::now();
    assert_eq!(d.tick(t0 + Duration::from_millis(300)), None);
    assert_eq!(d.current_dominant(), None);
}
