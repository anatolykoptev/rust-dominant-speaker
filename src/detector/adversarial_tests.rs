//! Adversarial / hard-red tests probing numeric instability, edge cases,
//! and invariants of the dominant-speaker detector.
//!
//! Goal: find bugs. Do not fix the library when a test fails here — the
//! failures document the bugs.

use super::*;
use crate::DetectorConfig;

#[cfg(test)]
use std::eprintln;

// ---------------------------------------------------------------------------
// 1. Numeric / math invariants (via the public detector surface).
// ---------------------------------------------------------------------------

/// `subunit_len_for(1)` must give 128 (ceil(128/1)), not panic or overflow.
/// Exercised indirectly through detector construction + tick.
#[test]
fn n1_equals_1_detector_functions() {
    let config = DetectorConfig {
        n1: 1,
        n2: 1,
        n3: 1,
        ..DetectorConfig::default()
    };
    let mut d: ActiveSpeakerDetector<u64> = ActiveSpeakerDetector::with_config(config);
    d.add_peer(1, 0);
    d.add_peer(2, 0);
    // Feed normal signals — must not panic.
    for i in 0..100 {
        let t: u64 = i * 20;
        d.record_level(1, 5, t);
        d.record_level(2, 127, t);
    }
    // Tick should succeed (returns Some or None, but must not panic).
    let _ = d.tick(2000);
}

/// `subunit_len_for(255)` = ceil(128/255) = 1. Exercise via detector.
#[test]
fn n1_equals_255_detector_functions() {
    let config = DetectorConfig {
        n1: 255,
        ..DetectorConfig::default()
    };
    let mut d: ActiveSpeakerDetector<u64> = ActiveSpeakerDetector::with_config(config);
    d.add_peer(1, 0);
    d.add_peer(2, 0);
    for i in 0..100 {
        let t: u64 = i * 20;
        d.record_level(1, 5, t);
        d.record_level(2, 127, t);
    }
    // Must not panic. Some election may occur; if it does, it must be 1.
    let change = d.tick(2000);
    if let Some(c) = change {
        assert_eq!(c.peer_id, 1, "loudest must win even with extreme n1");
    }
}

/// `binomial_coefficient(n, r)` with r > n. Currently returns 1 instead of 0.
/// This is a known mathematical issue — proves the bug exists in the public
/// activity-score path by constructing inputs that exercise the branch.
#[test]
fn binomial_coefficient_r_greater_than_n_returns_wrong_value() {
    use crate::numerics::binomial_coefficient;
    // Mathematically C(5, 10) = 0. If the impl returns 1 we have a bug.
    let got = binomial_coefficient(5, 10);
    assert_eq!(
        got, 0,
        "C(5, 10) must be 0 mathematically — got {got}. \
         This corrupts activity scores when v_l exceeds n_r."
    );
}

/// `compute_activity_score` must not panic with degenerate inputs where
/// v_l > n_r. Even if binomial_coefficient is wrong, the function does
/// `(n_r - v_l) as u32` — if v_l > n_r this is an arithmetic underflow
/// which panics in debug builds.
#[test]
fn compute_activity_score_handles_v_l_gt_n_r() {
    use crate::numerics::compute_activity_score;
    // n_r=5, v_l=10  —  5-10 underflows as u32.
    let result = std::panic::catch_unwind(|| compute_activity_score(10, 5, 0.5, 24.0));
    assert!(
        result.is_ok(),
        "compute_activity_score panicked when v_l > n_r (arithmetic underflow)"
    );
}

/// `compute_activity_score(0, 0, ...)` should not panic and should return a
/// sane (finite, >= MIN_ACTIVITY_SCORE) value.
#[test]
fn compute_activity_score_all_zero() {
    use crate::numerics::compute_activity_score;
    let s = compute_activity_score(0, 0, 0.5, 0.78);
    assert!(s.is_finite(), "score must be finite, got {s}");
    assert!(s >= 1.0e-10, "score must be >= MIN_ACTIVITY_SCORE, got {s}");
}

/// Zero-length `bigs` array fed to `compute_bigs`: `per = littles.len() / 0`
/// would divide by zero. Check this doesn't affect public paths.
/// This is covered indirectly because the public path never uses empty bigs —
/// but documenting here.
#[test]
fn compute_bigs_empty_bigs_is_internal_only() {
    // The public detector never calls compute_bigs with empty bigs, since
    // the mediums/longs arrays have fixed non-zero sizes. We document the
    // invariant here for completeness.
    use crate::numerics::compute_bigs;
    let littles = [0u8, 0, 0];
    let mut bigs = [0u8; 1];
    // This must not panic on normal inputs.
    let _ = compute_bigs(&littles, &mut bigs, 7);
}

// ---------------------------------------------------------------------------
// 2. Detector invariants.
// ---------------------------------------------------------------------------

fn feed(d: &mut ActiveSpeakerDetector, p: u64, lvl: u8, from_ms: u64, ms: u64) {
    let mut t = from_ms;
    let end = from_ms + ms;
    while t < end {
        d.record_level(p, lvl, t);
        t += 20;
    }
}

/// If peer A is consistently louder than peer B over N ticks, top_k must
/// always rank A above B — no transient inversion.
#[test]
fn score_monotonicity_louder_peer_always_ranks_higher() {
    let mut d = ActiveSpeakerDetector::new();
    d.add_peer(1, 0);
    d.add_peer(2, 0);

    // Feed loud peer 1, quiet peer 2 for multiple ticks, verifying ranking.
    let mut t: u64 = 0;
    let mut inversions = 0;
    for tick_i in 0..10 {
        for _ in 0..15 {
            d.record_level(1, 5, t); // loud
            d.record_level(2, 80, t); // quiet but not silent
            t += 20;
        }
        d.tick(t);
        let top = d.current_top_k(2);
        if top.len() == 2 && top[0] != 1 {
            inversions += 1;
            eprintln!("tick {tick_i}: unexpected ordering {top:?}");
        }
    }
    assert_eq!(
        inversions, 0,
        "consistently louder peer 1 was ranked below peer 2 on {inversions} ticks"
    );
}

/// After a speaker is elected, multiple consecutive ticks (with no new
/// audio) must return None — dominant must remain stable.
#[test]
fn dominance_stable_across_quiet_ticks() {
    let mut d = ActiveSpeakerDetector::new();
    d.add_peer(1, 0);
    d.add_peer(2, 0);
    feed(&mut d, 1, 5, 0, 2000);
    feed(&mut d, 2, 127, 0, 2000);
    let change = d.tick(2050);
    assert_eq!(change.map(|c| c.peer_id), Some(1));

    // Multiple silent ticks — incumbent must hold.
    let mut t: u64 = 2050;
    for i in 0..5 {
        t += 300;
        let out = d.tick(t);
        assert!(
            out.is_none(),
            "tick {i} triggered spurious speaker change: {out:?}"
        );
        assert_eq!(d.current_dominant(), Some(&1), "dominant must remain 1");
    }
}

/// `remove_peer(&999)` on a detector with no such peer must be a no-op.
#[test]
fn remove_nonexistent_peer_is_noop() {
    let mut d = ActiveSpeakerDetector::new();
    d.add_peer(1, 0);
    d.add_peer(2, 0);
    // Should not panic.
    d.remove_peer(&999);
    // Both peers must still be present.
    let scores = d.peer_scores();
    assert_eq!(scores.len(), 2);
}

/// `record_level(99, ..)` with no prior `add_peer` must auto-register so
/// peer 99 can participate in elections.
#[test]
fn record_level_auto_registers_peer() {
    let mut d = ActiveSpeakerDetector::new();
    d.add_peer(1, 0);
    feed(&mut d, 1, 127, 0, 2000); // silent
    feed(&mut d, 99, 5, 0, 2000); // loud; auto-registers
    let change = d.tick(2050);
    // peer 99 must be elected since it was the only loud one.
    assert_eq!(
        change.map(|c| c.peer_id),
        Some(99),
        "auto-registered peer 99 should win"
    );
    // peer 99 must now appear in peer_scores.
    let ids: Vec<u64> = d
        .peer_scores()
        .into_iter()
        .map(|(id, _, _, _)| id)
        .collect();
    assert!(ids.contains(&99));
}

/// Time going backwards (tick with earlier timestamp) must not panic.
#[test]
fn tick_with_earlier_time_does_not_panic() {
    let mut d = ActiveSpeakerDetector::new();
    let t0: u64 = 10_000; // 10 seconds in ms as anchor
    d.add_peer(1, t0);
    d.add_peer(2, t0);
    d.tick(t0);
    // Earlier tick. Should be a graceful no-op, or at least not panic.
    let earlier = t0 - 100;
    let result = std::panic::AssertUnwindSafe(|| d.tick(earlier));
    // Must not panic.
    let _ = result();
}

/// `record_level` with time going backwards: the speaker's `level_changed`
/// handles `now < last_level_change` by returning. Make sure it does not
/// panic and does not break subsequent state.
#[test]
fn record_level_with_earlier_time_does_not_panic() {
    let mut d = ActiveSpeakerDetector::new();
    let t0: u64 = 10_000; // 10 seconds in ms as anchor
    d.add_peer(1, t0);
    d.record_level(1, 5, t0 + 100);
    // Backwards in time:
    d.record_level(1, 5, t0);
    // Detector should still be in a valid state.
    let _ = d.tick(t0 + 200);
}

/// Calling `tick` 100 times at 10ms intervals must not panic and must not
/// flip the dominant speaker spuriously.
#[test]
fn rapid_ticks_do_not_flap() {
    let mut d = ActiveSpeakerDetector::new();
    d.add_peer(1, 0);
    d.add_peer(2, 0);
    feed(&mut d, 1, 5, 0, 2000);
    feed(&mut d, 2, 127, 0, 2000);
    let initial = d.tick(2050);
    assert_eq!(initial.map(|c| c.peer_id), Some(1));

    let mut t: u64 = 2050;
    let mut flaps = 0;
    for _ in 0..100 {
        t += 10;
        if let Some(c) = d.tick(t) {
            flaps += 1;
            eprintln!("unexpected speaker change: {:?}", c);
        }
    }
    assert_eq!(flaps, 0, "rapid ticks caused {flaps} spurious flaps");
    assert_eq!(d.current_dominant(), Some(&1));
}

/// All peers are silent (RFC 6464 level 127 = volume 0). Bootstrap election
/// still picks *some* peer. This verifies graceful degeneracy handling.
#[test]
fn all_peers_silent_bootstrap() {
    let mut d = ActiveSpeakerDetector::new();
    for id in 1..=3u64 {
        d.add_peer(id, 0);
    }
    // All silent.
    for id in 1..=3u64 {
        feed(&mut d, id, 127, 0, 2000);
    }
    let change = d.tick(2050);
    // Document behavior: bootstrap might elect someone (by tie-breaker) or None.
    // Either is acceptable as long as it doesn't panic and is consistent.
    // Verify current_dominant matches whatever happened.
    if let Some(c) = change {
        assert_eq!(
            Some(&c.peer_id),
            d.current_dominant(),
            "current_dominant must match the elected peer"
        );
    }
}

/// All peers equally loud — election must pick one and hold it.
#[test]
fn all_peers_equal_volume() {
    let mut d = ActiveSpeakerDetector::new();
    for id in 1..=4u64 {
        d.add_peer(id, 0);
    }
    for id in 1..=4u64 {
        feed(&mut d, id, 10, 0, 2000);
    }
    let change = d.tick(2050);
    // Some peer must be elected — not None (because all have equal activity
    // above zero and there's no incumbent).
    assert!(
        change.is_some(),
        "bootstrap election with equal-volume peers must produce a winner"
    );
    let first = change.unwrap().peer_id;
    // Subsequent tick without changes must keep the same dominant.
    let next = d.tick(2350);
    assert!(
        next.is_none(),
        "dominant must remain stable with equal input, got {next:?}"
    );
    assert_eq!(d.current_dominant(), Some(&first));
}

/// Elect peer 1 alone, then add peer 2 who is dramatically louder while
/// peer 1 stays silent. Peer 2 must eventually take over.
///
/// The existing `silence_then_speech_switches` test already covers the
/// simultaneous-registration case. This one exercises LATE-JOIN: peer 2 is
/// added after peer 1 is already the incumbent. With unlimited time the
/// challenger must clear C1/C2/C3 and win.
#[test]
fn single_then_louder_second_peer_wins() {
    let mut d = ActiveSpeakerDetector::new();
    d.add_peer(1, 0);
    // Feed peer 1 signal then long silence to give it a realistic low score.
    feed(&mut d, 1, 30, 0, 2000);
    let c = d.tick(2050);
    assert_eq!(c.map(|c| c.peer_id), Some(1));

    // Let peer 1 go silent for a while first, so its score decays
    // (avoid a freshly-elected incumbent with near-max medium score).
    let t_decay: u64 = 2050;
    feed(&mut d, 1, 127, t_decay, 3000);
    // Several ticks to settle.
    let mut t = t_decay;
    for _ in 0..10 {
        t += 300;
        d.tick(t);
    }

    // Now add peer 2 as the challenger. The adaptive min_level needs an
    // ordered descent of observations to track a floor it can then register
    // activity above. Start with a high level_raw (=quiet), let min_level
    // anchor there, then drop to loud.
    d.add_peer(2, t);
    let t1 = t;
    // First 200ms: "background" with level_raw=80 (volume=47) to anchor min_level low.
    let mut t_feed = t1;
    let phase1_end = t1 + 200;
    while t_feed < phase1_end {
        d.record_level(2, 80, t_feed);
        d.record_level(1, 127, t_feed);
        t_feed += 20;
    }
    // Then 8000ms loud (level_raw=5, volume=122), well above threshold.
    let phase2_end = t_feed + 8000;
    while t_feed < phase2_end {
        d.record_level(2, 5, t_feed);
        d.record_level(1, 127, t_feed);
        t_feed += 20;
    }

    let mut took_over = false;
    let mut t2 = t1;
    for _tick_i in 0..30 {
        t2 += 300;
        if let Some(ch) = d.tick(t2) {
            if ch.peer_id == 2 {
                took_over = true;
                break;
            }
        }
    }
    assert!(
        took_over,
        "much louder challenger (peer 2, level 0) should beat silent incumbent (peer 1). \
         scores = {:?}",
        d.peer_scores()
    );
}

/// 100-peer room: stress add/feed/tick with a varied level distribution.
#[test]
fn hundred_peer_room_no_panic() {
    let mut d = ActiveSpeakerDetector::new();
    for id in 0..100u64 {
        d.add_peer(id, 0);
    }
    let mut t: u64 = 0;
    for step in 0..120u64 {
        for id in 0..100u64 {
            // Pseudo-varied level per step/id.
            let lvl = ((id * 7 + step * 13) % 128) as u8;
            d.record_level(id, lvl, t);
        }
        t += 20;
    }
    // Tick — must not panic.
    let _ = d.tick(t + 20);
    // If we have a winner, it must be a valid registered peer.
    if let Some(dom) = d.current_dominant() {
        assert!(*dom < 100, "dominant {} is not a valid peer id", dom);
    }
}

/// Extreme config n1=n2=n3=1. Still must not panic and must elect someone
/// if there's only one peer.
#[test]
fn extreme_config_all_ones() {
    let config = DetectorConfig {
        n1: 1,
        n2: 1,
        n3: 1,
        ..DetectorConfig::default()
    };
    let mut d: ActiveSpeakerDetector<u64> = ActiveSpeakerDetector::with_config(config);
    d.add_peer(1, 0);
    // Single-peer election always succeeds.
    let c = d.tick(300);
    assert_eq!(c.map(|c| c.peer_id), Some(1));
}

/// Extreme config n2=1 with multi-peer + dynamic speech signal. Reaches
/// `compute_activity_score(mediums[0]=5, n_r=1)` which underflows.
/// This is the PUBLIC path for the v_l > n_r bug.
#[test]
fn extreme_config_small_n2_with_speech_must_not_panic() {
    let config = DetectorConfig {
        n2: 1,
        ..DetectorConfig::default()
    };
    let mut d: ActiveSpeakerDetector<u64> = ActiveSpeakerDetector::with_config(config);
    d.add_peer(1, 0);
    d.add_peer(2, 0);
    // Dynamic speech-like envelope that exercises min_level adaptation.
    let mut t: u64 = 0;
    for i in 0..300 {
        let lvl = if i % 6 == 0 { 80 } else { 5 };
        d.record_level(1, lvl, t);
        d.record_level(2, 127, t);
        t += 20;
    }
    // Must not panic on tick.
    let _ = d.tick(t);
}

/// Same bug, via n3=1 with enough time for the long window to fire.
#[test]
fn extreme_config_small_n3_must_not_panic() {
    let config = DetectorConfig {
        n3: 1,
        ..DetectorConfig::default()
    };
    let mut d: ActiveSpeakerDetector<u64> = ActiveSpeakerDetector::with_config(config);
    d.add_peer(1, 0);
    d.add_peer(2, 0);
    let mut t: u64 = 0;
    // Feed enough to fill the long window (~20s with 20ms samples).
    for i in 0..1500 {
        let lvl = if i % 6 == 0 { 80 } else { 5 };
        d.record_level(1, lvl, t);
        d.record_level(2, 127, t);
        t += 20;
    }
    let _ = d.tick(t);
}

/// Extreme config with n1=0: ensure it does not panic (subunit_len_for
/// guards against zero by clamping to 1).
#[test]
fn extreme_config_n1_zero_does_not_panic() {
    let config = DetectorConfig {
        n1: 0,
        ..DetectorConfig::default()
    };
    let mut d: ActiveSpeakerDetector<u64> = ActiveSpeakerDetector::with_config(config);
    d.add_peer(1, 0);
    d.add_peer(2, 0);
    for i in 0..100u64 {
        d.record_level(1, 5, i * 20);
        d.record_level(2, 127, i * 20);
    }
    let _ = d.tick(2000);
}

/// `current_top_k(0)` must return empty Vec and not panic.
#[test]
fn current_top_k_zero_returns_empty() {
    let mut d = ActiveSpeakerDetector::new();
    d.add_peer(1, 0);
    d.add_peer(2, 0);
    let top = d.current_top_k(0);
    assert!(top.is_empty());
}

/// `peer_scores()` before any tick must return entries with MIN_ACTIVITY_SCORE.
#[test]
fn peer_scores_before_any_tick() {
    let mut d = ActiveSpeakerDetector::new();
    d.add_peer(1, 0);
    d.add_peer(2, 0);
    let scores = d.peer_scores();
    assert_eq!(scores.len(), 2);
    for (_, imm, med, lng) in scores {
        // Should equal MIN_ACTIVITY_SCORE (1.0e-10).
        assert!((imm - 1.0e-10).abs() < 1e-20, "imm={imm}");
        assert!((med - 1.0e-10).abs() < 1e-20, "med={med}");
        assert!((lng - 1.0e-10).abs() < 1e-20, "lng={lng}");
    }
}

/// Remove all peers, then tick — must return None and not panic.
#[test]
fn tick_after_removing_all_peers() {
    let mut d = ActiveSpeakerDetector::new();
    d.add_peer(1, 0);
    d.add_peer(2, 0);
    feed(&mut d, 1, 5, 0, 1000);
    d.tick(1050);
    d.remove_peer(&1);
    d.remove_peer(&2);
    let c = d.tick(1350);
    assert_eq!(c, None);
    assert_eq!(d.current_dominant(), None);
}

/// Elect peer 1, remove peer 1, then re-add peer 1 — `current_dominant`
/// must have been cleared and a new tick should elect (if anyone is loud).
#[test]
fn dominance_clears_on_remove_then_readd() {
    let mut d = ActiveSpeakerDetector::new();
    d.add_peer(1, 0);
    d.add_peer(2, 0);
    feed(&mut d, 1, 5, 0, 2000);
    feed(&mut d, 2, 127, 0, 2000);
    let c = d.tick(2050);
    assert_eq!(c.map(|c| c.peer_id), Some(1));
    assert_eq!(d.current_dominant(), Some(&1));

    d.remove_peer(&1);
    assert_eq!(d.current_dominant(), None, "dominance must clear on remove");

    // Re-add peer 1. Dominance must still be None.
    let t1: u64 = 2100;
    d.add_peer(1, t1);
    assert_eq!(
        d.current_dominant(),
        None,
        "re-add should NOT restore lost dominance"
    );
}

/// Electing an incumbent, then getting a challenger with far higher score.
/// Verify `c2_margin` is non-negative and finite.
#[test]
fn c2_margin_always_finite_and_nonnegative() {
    let mut d = ActiveSpeakerDetector::new();
    d.add_peer(1, 0);
    d.add_peer(2, 0);
    feed(&mut d, 1, 5, 0, 2000);
    feed(&mut d, 2, 127, 0, 2000);
    let c = d.tick(2050).unwrap();
    assert!(
        c.c2_margin.is_finite(),
        "c2_margin not finite: {}",
        c.c2_margin
    );
    assert!(c.c2_margin >= 0.0, "c2_margin negative: {}", c.c2_margin);
    // Bootstrap election always has margin = 0.
    assert_eq!(c.c2_margin, 0.0);
}

// ---------------------------------------------------------------------------
// 3. Stress / fuzz.
// ---------------------------------------------------------------------------

/// Feed many random-ish levels to a 10-peer room, running 1000 ticks.
/// - No panic.
/// - current_dominant is always None OR one of the registered peers.
#[test]
fn stress_10_peers_1000_ticks() {
    let mut d = ActiveSpeakerDetector::new();
    for id in 0..10u64 {
        d.add_peer(id, 0);
    }
    // Simple deterministic PRNG: LCG.
    let mut state: u64 = 0xDEAD_BEEF;
    let mut next_u8 = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 56) as u8
    };

    let mut tick_t: u64 = 2000;
    for tick_i in 0..1000 {
        // Feed 10 samples per peer between ticks.
        for _ in 0..10 {
            for id in 0..10u64 {
                let lvl = next_u8() & 0x7F; // 0..127
                d.record_level(id, lvl, tick_t);
            }
            tick_t += 20;
        }
        // Tick must not panic.
        let _change = d.tick(tick_t);
        tick_t += 300;
        // If dominant exists, must be a valid id.
        if let Some(dom) = d.current_dominant() {
            assert!(*dom < 10, "tick {tick_i}: invalid dominant id {}", *dom);
        }
        // All scores must stay finite and >= MIN_ACTIVITY_SCORE.
        for (id, imm, med, lng) in d.peer_scores() {
            assert!(
                imm.is_finite() && med.is_finite() && lng.is_finite(),
                "tick {tick_i} peer {id}: non-finite score ({imm},{med},{lng})"
            );
            // Should never fall below MIN_ACTIVITY_SCORE.
            assert!(imm >= 1.0e-10, "imm score below floor: {imm}");
            assert!(med >= 1.0e-10, "med score below floor: {med}");
            assert!(lng >= 1.0e-10, "lng score below floor: {lng}");
        }
    }
}

/// SURPRISING BEHAVIOR: A peer fed constant maximum volume (level_raw=0,
/// volume=127) for 2 seconds produces activity scores frozen at
/// MIN_ACTIVITY_SCORE — because `min_level` latches to 127 on the first
/// sample and the threshold in `compute_immediates` saturates above any
/// possible level. Only the bootstrap tiebreaker (raw_level_sum) makes
/// elections work in this case; the activity score machinery is inert.
#[test]
fn constant_loud_signal_produces_zero_activity_score() {
    let mut d = ActiveSpeakerDetector::new();
    d.add_peer(1, 0);
    feed(&mut d, 1, 0, 0, 2000); // max volume (level_raw=0), constant
    d.tick(2050);
    d.tick(2350); // second tick to settle
    let scores = d.peer_scores();
    let (_, imm, med, lng) = scores[0];
    assert_eq!(
        imm, 1.0e-10,
        "constant-loud peer has immediate score at floor (min_level latch). got {imm}"
    );
    assert_eq!(
        med, 1.0e-10,
        "constant-loud peer has medium score at floor. got {med}"
    );
    assert_eq!(
        lng, 1.0e-10,
        "constant-loud peer has long score at floor. got {lng}"
    );
}

/// `current_dominant` must *always* be a peer that is currently registered.
#[test]
fn current_dominant_always_valid_after_removes() {
    let mut d = ActiveSpeakerDetector::new();
    for id in 1..=5u64 {
        d.add_peer(id, 0);
    }
    // Make peer 3 win.
    for id in 1..=5u64 {
        let lvl = if id == 3 { 5 } else { 127 };
        feed(&mut d, id, lvl, 0, 2000);
    }
    let c = d.tick(2050);
    assert_eq!(c.map(|c| c.peer_id), Some(3));

    // Remove a non-dominant peer — dominance must persist.
    d.remove_peer(&1);
    assert_eq!(d.current_dominant(), Some(&3));

    // Remove the dominant peer itself.
    d.remove_peer(&3);
    assert_eq!(d.current_dominant(), None);
}

/// Feed a peer with level 0 (max volume, volume=127 internally) via
/// `record_level` to trigger the upper corner of the activity formula.
/// Must not produce Inf/NaN scores.
#[test]
fn max_volume_produces_finite_scores() {
    let mut d = ActiveSpeakerDetector::new();
    d.add_peer(1, 0);
    d.add_peer(2, 0);
    feed(&mut d, 1, 0, 0, 3000); // max volume
    feed(&mut d, 2, 127, 0, 3000); // silence
    let _ = d.tick(3050);
    for (id, imm, med, lng) in d.peer_scores() {
        assert!(
            imm.is_finite() && med.is_finite() && lng.is_finite(),
            "peer {id} non-finite score ({imm},{med},{lng})"
        );
    }
}

/// `record_level` with level > 127 (above RFC 6464 cap): the code does
/// `level_raw.min(MAX_LEVEL)`, so 255 should be clamped to 127 (silence).
/// Must not panic.
#[test]
fn record_level_above_127_is_clamped() {
    let mut d = ActiveSpeakerDetector::new();
    d.add_peer(1, 0);
    d.add_peer(2, 0);
    // Peer 1: clearly loud. Peer 2: level 255 which should be treated as silence.
    feed(&mut d, 1, 5, 0, 2000);
    let mut t: u64 = 0;
    for _ in 0..100 {
        d.record_level(2, 255, t);
        t += 20;
    }
    let c = d.tick(2050);
    assert_eq!(
        c.map(|c| c.peer_id),
        Some(1),
        "clamped-silent peer 2 should not win over loud peer 1"
    );
}

/// After extreme bootstrap with all-equal peers, the elected peer must
/// appear in `current_top_k(1)`.
#[test]
fn top_k_1_matches_current_dominant() {
    let mut d = ActiveSpeakerDetector::new();
    for id in 1..=4u64 {
        d.add_peer(id, 0);
        feed(&mut d, id, 20, 0, 2000);
    }
    let c = d.tick(2050);
    let dom = c.map(|c| c.peer_id).expect("some peer must win");
    let top = d.current_top_k(1);
    assert_eq!(top.len(), 1);
    assert_eq!(
        top[0], dom,
        "current_top_k(1) ({}) must match current_dominant ({})",
        top[0], dom
    );
}
