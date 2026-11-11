# Contributing

Thanks for your interest. Open an issue first for anything beyond a small fix, so
the approach can be agreed. The crate's scope is deliberately narrow: a
sans-I/O implementation of the dominant speaker algorithm, faithful to the
mediasoup and Jitsi baseline.

## Build and test

Requires Rust 1.75 or newer.

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo build --target wasm32-unknown-unknown --no-default-features   # no_std check
cargo bench                                                          # optional
```

CI runs all of these on every push and pull request, and the tests also on beta
and on Rust 1.75.

## Guidelines

- Behaviour that differs from the mediasoup and Jitsi baseline must be opt-in.
- Keep the crate `no_std`-compatible and free of I/O, async and WebRTC stack
  dependencies.
- Add a test for every behaviour change; edge cases belong in
  `src/detector/adversarial_tests.rs`.
- Record user-visible changes under `[Unreleased]` in [CHANGELOG.md](CHANGELOG.md).

## Commits and pull requests

Use Conventional Commits (`feat:`, `fix:`, `docs:`, `chore:`, `refactor:`), as in
the existing history. Describe what changed and why; the pull request template
lists the checks. By contributing you agree that your work is licensed under
MIT OR Apache-2.0, like the rest of the crate.
