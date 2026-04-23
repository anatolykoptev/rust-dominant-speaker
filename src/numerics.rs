//! Pure math helpers ported from mediasoup's ActiveSpeakerObserver.
//!
//! No state — every function is a deterministic mapping of inputs to
//! outputs, making the algorithm core unit-testable without any
//! runtime scaffolding.

use super::MIN_ACTIVITY_SCORE;

/// Compute the binomial coefficient C(n, r).
///
/// Returns 0 when `r < 0` or `r > n` (mathematically correct).
///
/// Port of mediasoup C++ `ComputeBinomialCoefficient`.
pub(crate) fn binomial_coefficient(n: i32, r: i32) -> i64 {
    if r < 0 || r > n {
        return 0;
    }
    // Use the larger of r and n-r to reduce loop iterations.
    let r = r.max(n - r);
    let (mut t, mut i, mut j): (i64, i64, i64) = (1, n as i64, 1);
    while i > r as i64 {
        t = t * i / j;
        i -= 1;
        j += 1;
    }
    t
}

/// Compute the log-domain activity score for one time-scale window.
///
/// `v_l` — count of active subbands; `n_r` — window size;
/// `p` — binomial success probability; `lambda` — Poisson rate.
///
/// Returns at least [`MIN_ACTIVITY_SCORE`] to avoid log(0) downstream.
///
/// Port of mediasoup C++ `ComputeActivityScore`.
pub(crate) fn compute_activity_score(v_l: u8, n_r: u32, p: f64, lambda: f64) -> f64 {
    // Clamp v_l to n_r: more active subbands than window size is impossible
    // under correct usage, but a misconfigured n2/n3 can cause compute_bigs
    // to produce v_l > n_r, leading to unsigned underflow in (n_r - v_l).
    let v_l = (v_l as u32).min(n_r);
    let bc = binomial_coefficient(n_r as i32, v_l as i32).max(1) as f64;
    let s = bc.ln() + (v_l as f64) * p.ln() + ((n_r - v_l) as f64) * (1.0 - p).ln() - lambda.ln()
        + lambda * (v_l as f64);
    s.max(MIN_ACTIVITY_SCORE)
}

/// Downsample a `littles` array into `bigs` by counting samples per bucket
/// that exceed `threshold`. Returns `true` if any bucket changed.
///
/// Port of mediasoup C++ `ComputeBigs`.
pub(crate) fn compute_bigs(littles: &[u8], bigs: &mut [u8], threshold: u8) -> bool {
    let per = littles.len() / bigs.len();
    let mut changed = false;
    let mut l = 0usize;
    for slot in bigs.iter_mut() {
        let mut sum: u8 = 0;
        let end = l + per;
        while l < end {
            if littles[l] > threshold {
                sum += 1;
            }
            l += 1;
        }
        if *slot != sum {
            *slot = sum;
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::super::N1;
    use super::*;

    #[test]
    fn binomial_and_activity_score_sanity() {
        assert_eq!(binomial_coefficient(20, 10), 184_756);
        // All-silent sample floors at MIN_ACTIVITY_SCORE.
        let s = compute_activity_score(0, N1, 0.5, 0.78);
        assert!((s - MIN_ACTIVITY_SCORE).abs() < 1e-20, "got {s}");
    }

    /// Regression: C(n, r) must be 0 when r > n, not 1.
    /// The old code returned 1 because the loop never executed.
    #[test]
    fn binomial_r_greater_than_n_is_zero() {
        assert_eq!(binomial_coefficient(5, 10), 0);
        assert_eq!(binomial_coefficient(0, 1), 0);
        assert_eq!(binomial_coefficient(13, 14), 0);
    }

    #[test]
    fn binomial_negative_r_is_zero() {
        assert_eq!(binomial_coefficient(5, -1), 0);
    }

    /// Regression: compute_activity_score must not panic when v_l > n_r.
    /// The old code did unsigned subtraction (n_r - v_l as u32) which panics
    /// in debug mode and wraps to ~4 billion in release mode.
    #[test]
    fn activity_score_v_l_greater_than_n_r_no_panic() {
        // v_l=5, n_r=1 — was a panic in debug, wraparound in release.
        let s = compute_activity_score(5, 1, 0.5, 24.0);
        assert!(s.is_finite(), "score must be finite, got {s}");
        assert!(s >= MIN_ACTIVITY_SCORE, "score must be >= MIN, got {s}");
    }

    #[test]
    fn activity_score_v_l_equals_n_r_no_panic() {
        // Edge: v_l == n_r — (n_r - v_l) == 0, must compute correctly.
        let s = compute_activity_score(13, 13, 0.5, 0.78);
        assert!(s.is_finite(), "got {s}");
    }

    #[test]
    fn compute_bigs_counts_above_threshold() {
        let littles = [0u8, 8, 9, 10, 0, 0, 0, 0, 0, 0];
        let mut bigs = [0u8; 2];
        // bucket 0: {0,8,9,10,0} — three samples > 7 (8, 9, 10) → 3.
        // bucket 1: all zeros → 0.
        assert!(compute_bigs(&littles, &mut bigs, 7));
        assert_eq!(bigs, [3, 0]);
        // Running again with the same data must NOT report change.
        assert!(!compute_bigs(&littles, &mut bigs, 7));
    }
}
