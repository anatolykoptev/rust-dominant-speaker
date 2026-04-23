//! Criterion benchmark for `ActiveSpeakerDetector::tick`.
//!
//! Two scenarios at 300ms tick cadence (setup is outside the measurement loop):
//! - `tick_50_peers` — high-contention: 25 loud peers (RFC 6464 level 10)
//!   vs 25 quiet peers (level 100); exercises the full election path.
//! - `tick_5_peers` — low-contention baseline: 1 dominant speaker (level 5)
//!   vs 4 quiet peers (level 90).

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use dominant_speaker::ActiveSpeakerDetector;

fn bench_tick_50_peers(c: &mut Criterion) {
    let mut d = ActiveSpeakerDetector::new();

    // Register 50 peers with varied audio levels.
    for i in 0u64..50 {
        d.add_peer(i, 0);
        // Alternate: even peers speak at level 10, odd peers at 100.
        let level: u8 = if i % 2 == 0 { 10 } else { 100 };
        let mut t_ms: u64 = 0;
        while t_ms < 2000 {
            d.record_level(i, level, t_ms);
            t_ms += 20;
        }
    }

    // Warm up: one tick before benchmarking.
    let _ = d.tick(2000);

    let mut tick_ms: u64 = 2300;
    c.bench_function("tick_50_peers", |b| {
        b.iter(|| {
            tick_ms += 300;
            black_box(d.tick(tick_ms))
        })
    });
}

fn bench_tick_5_peers(c: &mut Criterion) {
    let mut d = ActiveSpeakerDetector::new();

    for i in 0u64..5 {
        d.add_peer(i, 0);
        let level: u8 = if i == 0 { 5 } else { 90 };
        let mut t_ms: u64 = 0;
        while t_ms < 2000 {
            d.record_level(i, level, t_ms);
            t_ms += 20;
        }
    }
    let _ = d.tick(2000);

    let mut tick_ms: u64 = 2300;
    c.bench_function("tick_5_peers", |b| {
        b.iter(|| {
            tick_ms += 300;
            black_box(d.tick(tick_ms))
        })
    });
}

criterion_group!(benches, bench_tick_50_peers, bench_tick_5_peers);
criterion_main!(benches);
