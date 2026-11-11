# rust-dominant-speaker — notes for coding agents

Index of every document: [docs/INDEX.md](docs/INDEX.md).

## What this is

A `no_std`-capable Rust library (crate `rust-dominant-speaker`, library name
`dominant_speaker`) implementing the Volfin and Cohen dominant speaker
identification algorithm as used by Jitsi and mediasoup. It is sans-I/O: callers
pass RFC 6464 audio levels and `u64` millisecond timestamps.

## Layout

`src/lib.rs` public API and crate docs; `src/detector.rs` and `src/detector/`
(the detector, its tick logic and tests, including `adversarial_tests.rs`);
`src/speaker.rs` per-peer state; `src/numerics.rs` the activity-score math;
`benches/tick.rs` the Criterion benchmark.

## Gate

CI (`.github/workflows/ci.yml`) runs five jobs; reproduce them with:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features        # CI also runs it on beta and on 1.75
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo build --target wasm32-unknown-unknown --no-default-features
```

`deny.toml` configures cargo-deny. Tagging `v*` publishes to crates.io through
`publish.yml` (Trusted Publishing).

## Invariants easy to break

- Algorithm constants follow mediasoup's production tuning (table in the README);
  a behavioural change must be opt-in through configuration, never a silent
  change of defaults.
- The crate must stay `no_std` with `default-features = false`: no `std`-only
  API outside `#[cfg(feature = "std")]`, and the wasm32 build above must pass.
- Time is a caller-supplied `u64` in milliseconds; do not reintroduce
  `std::time::Instant` in the API (the 0.3.0 breaking change).
- MSRV is 1.75: avoid newer language and library features, and keep `Cargo.toml`
  `rust-version`, the CI matrix and the README in step.
- Keep the dependency set minimal (`hashbrown`, `libm`, optional `serde`).
