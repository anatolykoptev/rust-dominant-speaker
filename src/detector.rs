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

use std::collections::BTreeMap;
use std::time::Instant;

use super::speaker::Speaker;
use super::{DetectorConfig, LEVEL_IDLE_TIMEOUT_MS, MAX_LEVEL, MIN_LEVEL, SPEAKER_IDLE_TIMEOUT_MS};

#[cfg(test)]
mod tests;

/// Per-room dominant-speaker detector.
///
/// Feed it RFC 6464 audio-level observations via [`record_level`](Self::record_level),
/// then call [`tick`](Self::tick) on a 300ms timer. The detector returns
/// `Some(peer_id)` only when the dominant speaker changes.
///
/// Uses `BTreeMap` rather than `HashMap` so the bootstrap "seed" pick in
/// the internal election is deterministic — mediasoup's C++ impl uses
/// `std::map`, which is also ordered. The number of peers per room is
/// small (tens), so the O(log n) cost is negligible.
#[derive(Debug)]
pub struct ActiveSpeakerDetector {
    config: DetectorConfig,
    speakers: BTreeMap<u64, Speaker>,
    current_dominant: Option<u64>,
    last_level_idle_time: Option<Instant>,
}

impl Default for ActiveSpeakerDetector {
    fn default() -> Self {
        Self::with_config(DetectorConfig::default())
    }
}

impl ActiveSpeakerDetector {
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
            speakers: BTreeMap::new(),
            current_dominant: None,
            last_level_idle_time: None,
        }
    }

    /// Return the active detector configuration.
    pub fn config(&self) -> &DetectorConfig {
        &self.config
    }

    /// Register a peer. Idempotent — calling again for an existing peer is a no-op.
    pub fn add_peer(&mut self, peer_id: u64, now: Instant) {
        self.speakers
            .entry(peer_id)
            .or_insert_with(|| Speaker::new(now));
    }

    /// Remove a peer. If the removed peer was dominant, dominance is cleared
    /// and the next [`tick`](Self::tick) will elect a new speaker.
    pub fn remove_peer(&mut self, peer_id: u64) {
        self.speakers.remove(&peer_id);
        if self.current_dominant == Some(peer_id) {
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
    pub fn record_level(&mut self, peer_id: u64, level_raw: u8, now: Instant) {
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
        let dom = self.current_dominant;
        for (&id, sp) in self.speakers.iter_mut() {
            let idle = now.duration_since(sp.last_level_change).as_millis() as u64;
            if SPEAKER_IDLE_TIMEOUT_MS < idle && dom != Some(id) {
                sp.paused = true;
            } else if LEVEL_IDLE_TIMEOUT_MS < idle {
                sp.level_changed(MIN_LEVEL, now);
            }
        }
    }

    /// Advance the detector clock to `now`.
    ///
    /// Returns `Some(peer_id)` when the dominant speaker changes; `None`
    /// when the incumbent holds. Call this on a [`TICK_INTERVAL`](crate::TICK_INTERVAL)
    /// timer (300ms).
    pub fn tick(&mut self, now: Instant) -> Option<u64> {
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
    fn calculate_active_speaker(&mut self) -> Option<u64> {
        let new_id = if self.speakers.len() == 1 {
            self.speakers.keys().next().copied()
        } else {
            let incumbent = self.current_dominant;
            // Bootstrap: arbitrary seed when no incumbent — any real
            // activity will overwrite via the ratio test below.
            let seed = incumbent.or_else(|| self.speakers.keys().next().copied())?;
            if let Some(s) = self.speakers.get_mut(&seed) {
                s.eval_scores(self.config.n1, self.config.n2, self.config.n3);
            }
            let dom = {
                let s = self.speakers.get(&seed)?;
                [s.score(0), s.score(1), s.score(2)]
            };
            let mut best_c2 = self.config.c2;
            let mut winner: Option<u64> = if incumbent.is_none() {
                Some(seed)
            } else {
                None
            };
            let ids: Vec<u64> = self.speakers.keys().copied().collect();
            for id in ids {
                if Some(id) == incumbent {
                    continue;
                }
                let Some(sp) = self.speakers.get_mut(&id) else {
                    continue;
                };
                if sp.paused {
                    continue;
                }
                sp.eval_scores(self.config.n1, self.config.n2, self.config.n3);
                let c1 = (sp.score(0) / dom[0]).ln();
                let c2 = (sp.score(1) / dom[1]).ln();
                let c3 = (sp.score(2) / dom[2]).ln();
                if c1 > self.config.c1 && c2 > self.config.c2 && c3 > self.config.c3 && c2 > best_c2
                {
                    best_c2 = c2;
                    winner = Some(id);
                }
            }
            winner
        };
        match (new_id, self.current_dominant) {
            (Some(n), Some(c)) if n == c => None,
            (Some(n), _) => {
                self.current_dominant = Some(n);
                Some(n)
            }
            _ => None,
        }
    }

    /// Return the current dominant peer ID, if any.
    ///
    /// This is a read-only snapshot; dominance only changes via [`tick`](Self::tick).
    pub fn current_dominant(&self) -> Option<u64> {
        self.current_dominant
    }
}
