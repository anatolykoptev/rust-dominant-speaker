//! Room-level dominant-speaker detector — owns a [`Speaker`] per peer and
//! runs mediasoup's hysteresis election on every tick.
//!
//! # Public surface
//! - [`ActiveSpeakerDetector::new`]
//! - [`ActiveSpeakerDetector::add_peer`]
//! - [`ActiveSpeakerDetector::remove_peer`]
//! - [`ActiveSpeakerDetector::record_level`]
//! - [`ActiveSpeakerDetector::tick`]
//! - [`ActiveSpeakerDetector::current_dominant`]
//! - [`ActiveSpeakerDetector::current_top_k`]
//! - [`ActiveSpeakerDetector::peer_scores`]

use std::collections::HashMap;
use std::hash::Hash;
use std::time::Instant;

use super::speaker::Speaker;
use super::{
    subunit_len_for, DetectorConfig, SpeakerChange, LEVEL_IDLE_TIMEOUT_MS, MAX_LEVEL, MIN_LEVEL,
    SPEAKER_IDLE_TIMEOUT_MS,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod adversarial_tests;

/// Per-room dominant-speaker detector.
///
/// Generic over `PeerId` — any type that is `Eq + Hash + Clone` works:
/// `u64`, `String`, `Uuid`, custom newtypes. The default is `u64` for
/// backward compatibility with v0.1.x.
///
/// Feed RFC 6464 audio levels via [`record_level`](Self::record_level),
/// then call [`tick`](Self::tick) on a 300ms timer.
#[derive(Debug)]
pub struct ActiveSpeakerDetector<PeerId = u64> {
    config: DetectorConfig,
    speakers: HashMap<PeerId, Speaker>,
    current_dominant: Option<PeerId>,
    last_level_idle_time: Option<Instant>,
}

impl<PeerId> Default for ActiveSpeakerDetector<PeerId>
where
    PeerId: Eq + Hash + Clone,
{
    fn default() -> Self {
        Self::with_config(DetectorConfig::default())
    }
}

impl<PeerId> ActiveSpeakerDetector<PeerId>
where
    PeerId: Eq + Hash + Clone,
{
    /// Create a new empty detector with default (mediasoup-identical) constants.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a detector with custom tuning constants.
    ///
    /// See [`DetectorConfig`] for available parameters and their defaults.
    pub fn with_config(config: DetectorConfig) -> Self {
        Self {
            config,
            speakers: HashMap::new(),
            current_dominant: None,
            last_level_idle_time: None,
        }
    }

    /// Return the active detector configuration.
    pub fn config(&self) -> &DetectorConfig {
        &self.config
    }

    /// Register a peer. Idempotent — calling again for an existing peer is a no-op.
    pub fn add_peer(&mut self, peer_id: PeerId, now: Instant) {
        self.speakers
            .entry(peer_id)
            .or_insert_with(|| Speaker::new(now));
    }

    /// Remove a peer. If the removed peer was dominant, dominance is cleared
    /// and the next [`tick`](Self::tick) will elect a new speaker.
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.speakers.remove(peer_id);
        if self.current_dominant.as_ref() == Some(peer_id) {
            self.current_dominant = None;
        }
    }

    /// Record an RFC 6464 audio-level observation for a peer.
    ///
    /// `level_raw` is the raw RFC 6464 value: 0 = loudest, 127 = silent.
    /// The detector converts to volume internally (`volume = 127 − level_raw`),
    /// matching mediasoup's convention.
    ///
    /// If the peer was not previously added via [`add_peer`](Self::add_peer),
    /// it is registered implicitly.
    pub fn record_level(&mut self, peer_id: PeerId, level_raw: u8, now: Instant) {
        let vol = MAX_LEVEL.saturating_sub(level_raw.min(MAX_LEVEL));
        self.speakers
            .entry(peer_id)
            .or_insert_with(|| Speaker::new(now))
            .level_changed(vol, now);
    }

    /// Replace stale level entries with silence for idle peers.
    ///
    /// Port of mediasoup C++ `TimeoutIdleLevels`.
    fn timeout_idle_levels(&mut self, now: Instant) {
        let dom = self.current_dominant.clone();
        for (id, sp) in self.speakers.iter_mut() {
            let idle = now.duration_since(sp.last_level_change).as_millis() as u64;
            if SPEAKER_IDLE_TIMEOUT_MS < idle && dom.as_ref() != Some(id) {
                sp.paused = true;
            } else if LEVEL_IDLE_TIMEOUT_MS < idle {
                sp.level_changed(MIN_LEVEL, now);
            }
        }
    }

    /// Advance the detector clock to `now`.
    ///
    /// Returns `Some(SpeakerChange)` when the dominant speaker changes; `None`
    /// when the incumbent holds. Call on a [`TICK_INTERVAL`](crate::TICK_INTERVAL) timer.
    pub fn tick(&mut self, now: Instant) -> Option<SpeakerChange<PeerId>> {
        match self.last_level_idle_time {
            Some(t) if now.duration_since(t).as_millis() as u64 >= LEVEL_IDLE_TIMEOUT_MS => {
                self.timeout_idle_levels(now);
                self.last_level_idle_time = Some(now);
            }
            None => self.last_level_idle_time = Some(now),
            _ => {}
        }
        if self.speakers.is_empty() {
            return None;
        }
        self.calculate_active_speaker()
    }

    /// Run mediasoup's `CalculateActiveSpeaker` hysteresis election.
    ///
    /// A challenger must beat the incumbent on all three log-ratios (C1/C2/C3)
    /// AND have the highest medium ratio in the room to win.
    fn calculate_active_speaker(&mut self) -> Option<SpeakerChange<PeerId>> {
        let subunit_len = subunit_len_for(self.config.n1);

        let (new_id, c2_margin) = if self.speakers.len() == 1 {
            let only = self.speakers.keys().next().cloned()?;
            (Some(only), 0.0f64)
        } else {
            let incumbent = self.current_dominant.clone();

            // Eval scores for all non-paused speakers.
            let ids: Vec<PeerId> = self.speakers.keys().cloned().collect();
            for id in &ids {
                let Some(sp) = self.speakers.get_mut(id) else {
                    continue;
                };
                if sp.paused {
                    continue;
                }
                sp.eval_scores(self.config.n1, self.config.n2, self.config.n3, subunit_len);
            }

            match incumbent {
                None => {
                    // Bootstrap: no incumbent yet — elect the peer with the
                    // highest medium-window score. Use raw level sum as a
                    // tiebreaker when scores are equal (e.g., before min_level
                    // adaptation has converged after a rapid volume change).
                    let mut best_score = f64::NEG_INFINITY;
                    let mut best_raw: u32 = 0;
                    let mut winner: Option<PeerId> = None;
                    for id in &ids {
                        let Some(sp) = self.speakers.get(id) else {
                            continue;
                        };
                        if sp.paused {
                            continue;
                        }
                        let s = sp.score(1);
                        let raw = sp.raw_level_sum();
                        if s > best_score || (s == best_score && raw > best_raw) {
                            best_score = s;
                            best_raw = raw;
                            winner = Some(id.clone());
                        }
                    }
                    (winner, 0.0)
                }
                Some(ref inc) => {
                    // Incumbent exists: a challenger must beat it on all three
                    // log-ratio thresholds (C1/C2/C3) — mediasoup hysteresis.
                    let dom = {
                        let s = self.speakers.get(inc)?;
                        [s.score(0), s.score(1), s.score(2)]
                    };
                    let mut best_c2 = self.config.c2;
                    let mut winner: Option<PeerId> = None;
                    for id in ids {
                        if &id == inc {
                            continue;
                        }
                        let Some(sp) = self.speakers.get(&id) else {
                            continue;
                        };
                        if sp.paused {
                            continue;
                        }
                        let c1 = (sp.score(0) / dom[0]).ln();
                        let c2 = (sp.score(1) / dom[1]).ln();
                        let c3 = (sp.score(2) / dom[2]).ln();
                        if c1 > self.config.c1
                            && c2 > self.config.c2
                            && c3 > self.config.c3
                            && c2 > best_c2
                        {
                            best_c2 = c2;
                            winner = Some(id);
                        }
                    }
                    let margin = (best_c2 - self.config.c2).max(0.0);
                    (winner, margin)
                }
            }
        };

        match (new_id, &self.current_dominant) {
            (Some(n), Some(c)) if n == *c => None,
            (Some(n), _) => {
                self.current_dominant = Some(n.clone());
                Some(SpeakerChange {
                    peer_id: n,
                    c2_margin,
                })
            }
            _ => None,
        }
    }

    /// Return the current dominant peer ID, if any.
    ///
    /// This is a read-only snapshot; dominance only changes via [`tick`](Self::tick).
    pub fn current_dominant(&self) -> Option<&PeerId> {
        self.current_dominant.as_ref()
    }

    /// Return the top `k` speakers by medium-window activity score, highest first.
    ///
    /// Paused (idle) peers are excluded. Returns fewer than `k` entries if
    /// fewer active peers exist. Scores are stale between ticks — call after
    /// each [`tick`](Self::tick) for current data.
    ///
    /// Ties in medium score are broken by raw level sum (higher = louder),
    /// matching the bootstrap-election tiebreaker in [`tick`](Self::tick).
    pub fn current_top_k(&self, k: usize) -> Vec<PeerId> {
        let mut scored: Vec<(&PeerId, f64, u32)> = self
            .speakers
            .iter()
            .filter(|(_, s)| !s.paused)
            .map(|(id, s)| (id, s.medium_score, s.raw_level_sum()))
            .collect();
        // Sort descending: primary key = medium_score, tiebreaker = raw_level_sum.
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.2.cmp(&a.2))
        });
        scored
            .into_iter()
            .take(k)
            .map(|(id, _, _)| id.clone())
            .collect()
    }

    /// Return all peers with their current `(peer_id, immediate, medium, long)` scores.
    ///
    /// Scores are stale between ticks — call after each [`tick`](Self::tick) for
    /// current data. Useful for dashboards, custom layer selectors, and debugging.
    /// Order is unspecified.
    pub fn peer_scores(&self) -> Vec<(PeerId, f64, f64, f64)> {
        self.speakers
            .iter()
            .map(|(id, s)| (id.clone(), s.immediate_score, s.medium_score, s.long_score))
            .collect()
    }

    #[cfg(test)]
    pub(super) fn speakers_mut(&mut self) -> &mut HashMap<PeerId, Speaker> {
        &mut self.speakers
    }
}
