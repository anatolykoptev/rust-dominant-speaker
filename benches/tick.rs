//! Criterion benchmark for `ActiveSpeakerDetector::tick`.
//!
//! Measures: 50-peer room at 300ms tick cadence, all peers active.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use dominant_speaker::ActiveSpeakerDetector;
use std::time::{Duration, Instant};

fn bench_tick_50_peers(c: &mut Criterion) {
    let mut d = ActiveSpeakerDetector::new();
    let t0 = Instant::now();

    // Register 50 peers with varied audio levels.
    for i in 0u64..50 {
        d.add_peer(i, t0);
        // Alternate: even peers speak at level 10, odd peers at 100.
        let level: u8 = if i % 2 == 0 { 10 } else { 100 };
        let mut t = t0;
        let end = t0 + Duration::from_millis(2000);
        while t < end {
            d.record_level(i, level, t);
            t += Duration::from_millis(20);
        }
    }

    // Warm up: one tick before benchmarking.
    let _ = d.tick(t0 + Duration::from_millis(2000));

    let mut tick_time = t0 + Duration::from_millis(2300);
    c.bench_function("tick_50_peers", |b| {
        b.iter(|| {
            tick_time += Duration::from_millis(300);
            black_box(d.tick(tick_time))
        })
    });
}

fn bench_tick_5_peers(c: &mut Criterion) {
    let mut d = ActiveSpeakerDetector::new();
    let t0 = Instant::now();

    for i in 0u64..5 {
        d.add_peer(i, t0);
        let level: u8 = if i == 0 { 5 } else { 90 };
        let mut t = t0;
        while t < t0 + Duration::from_millis(2000) {
            d.record_level(i, level, t);
            t += Duration::from_millis(20);
        }
    }
    let _ = d.tick(t0 + Duration::from_millis(2000));

    let mut tick_time = t0 + Duration::from_millis(2300);
    c.bench_function("tick_5_peers", |b| {
        b.iter(|| {
            tick_time += Duration::from_millis(300);
            black_box(d.tick(tick_time))
        })
    });
}

criterion_group!(benches, bench_tick_50_peers, bench_tick_5_peers);
criterion_main!(benches);
