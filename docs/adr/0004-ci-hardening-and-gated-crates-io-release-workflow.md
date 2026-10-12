# 0004. CI hardening & gated crates.io release workflow

## Status

Accepted (implemented in [#15](https://github.com/davidfowl/razor-rust/pull/15)).

## Context

[#7](https://github.com/davidfowl/razor-rust/issues/7) ("Epic: CI/CD")
scoped a GitHub Actions workflow running `cargo build`/`test`/`clippy -D
warnings`/`fmt --check` on PRs and `main` (landed in #12), plus a release
workflow for publishing to crates.io — explicitly "gated, not automatic"
given the risk of an accidental or premature publish.

The tradeoffs addressed in the follow-up PR:

- Whether doc warnings/broken intra-doc links should fail CI (vs. being
  silently ignored) — relevant to keeping docs current per #9's convention.
- Whether to pin and verify an explicit MSRV (`rust-version` in
  `Cargo.toml`) in CI, vs. relying on whatever toolchain happens to be
  installed, risking silent drift from the documented minimum.
- How permissive the crates.io release workflow should be: fully automatic
  publish on a trigger vs. a dry-run-by-default, explicitly-gated manual
  publish path.

## Decision

- Added a `docs` CI job (`cargo doc --no-deps --document-private-items` with
  `RUSTDOCFLAGS=-D warnings`), so doc warnings/broken links now fail CI.
- Added an `msrv` CI job that reads `rust-version` from `Cargo.toml`
  (`1.75`) and builds + tests against that exact toolchain, so a PR relying
  on a newer language/stdlib feature fails CI instead of silently breaking
  the documented MSRV.
- `build-and-test` now also runs the `hello` example, so sample code is
  exercised, not just compiled.
- Added `.github/workflows/release.yml`: `workflow_dispatch`-triggered,
  dry-run (`cargo publish --dry-run`) by default. A real publish requires
  **both** an explicit `dry_run: false` input **and** a
  `CARGO_REGISTRY_TOKEN` repository secret — satisfying #7's "gated, not
  automatic" requirement without blocking on when the project is actually
  ready to publish.

This was judged a routine-but-consequential CI/tooling decision made
directly against #7's existing scope, rather than a hard-to-reverse
architecture tradeoff requiring maintainer sign-off before proceeding.

## Consequences

- CI now fails on doc warnings and MSRV drift, in addition to the existing
  fmt/clippy/build/test checks.
- Publishing to crates.io stays a deliberate, two-condition manual action
  until a `CARGO_REGISTRY_TOKEN` secret is added and a non-dry-run
  `workflow_dispatch` is explicitly requested.
- CI does not yet exercise `--all-features` (relevant once the `tokio`
  feature from [0002](./0002-async-runtime-dependency-for-streaming-rendering.md)
  needs coverage) — a known follow-up noted on PR #17, owned by this epic
  (#7).
