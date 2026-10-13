# Contributor instructions

This guide applies to the whole repository and to both human and AI
contributors. Razor Slices for Rust is an early-stage port of
[RazorSlices](https://github.com/DamianEdwards/RazorSlices), not a complete
template engine yet. Use [README.md](README.md) for the runnable demo and
[#1](https://github.com/davidfowl/razor-rust/issues/1) for live roadmap scope.
Refresh main, the relevant issues, and active PR ownership before starting;
do not duplicate work already in progress.

## Repository map

| Path | Purpose |
| --- | --- |
| [Cargo.toml](Cargo.toml) | Workspace membership and shared package metadata, including MSRV |
| [rust-toolchain.toml](rust-toolchain.toml) | Default development toolchain: stable |
| [crates/razor-slices/](crates/razor-slices/) | The only workspace crate today: rendering runtime |
| [src/lib.rs](crates/razor-slices/src/lib.rs) | Public runtime exports |
| [src/slice.rs](crates/razor-slices/src/slice.rs) | Synchronous `Slice`, render errors, reference/box implementations, and unit tests |
| [src/html.rs](crates/razor-slices/src/html.rs) | Shared HTML encoding, `HtmlString`, `WriteHtmlValue`, and unit tests |
| [src/async_slice.rs](crates/razor-slices/src/async_slice.rs) | Async writer trait, buffered rendering bridge, optional Tokio adapter, and tests |
| [examples/hello.rs](crates/razor-slices/examples/hello.rs) | Runnable hand-written `Slice` demo |
| [examples/html_encode_bench.rs](crates/razor-slices/examples/html_encode_bench.rs) | Existing HTML encoder microbenchmark |
| [.github/workflows/ci.yml](.github/workflows/ci.yml) | Stable and MSRV CI, with default/all-features coverage |
| [.github/workflows/release.yml](.github/workflows/release.yml) | Manual, dry-run-by-default crates.io release workflow |
| [docs/adr/README.md](docs/adr/README.md) | Architecture decision index, status, numbering, and template |

## Current implementation boundaries

- `Slice` is synchronous and hand-implementable; `render` writes to
  `std::fmt::Write`, with convenience methods returning `String` and
  `Vec<u8>`. Keep the existing trait-object use cases in mind when changing
  the runtime.
- `render_async` currently calls `render_to_bytes`, buffers the whole output,
  then calls the destination's `write_all` and `flush`. It is an async I/O
  bridge, not incremental mid-template streaming. That remaining work is
  tracked in [#11](https://github.com/davidfowl/razor-rust/issues/11).
- `AsyncSliceWriter` is runtime-agnostic. The default-off `tokio` feature adds
  `TokioWriter`; the core has no mandatory async-runtime dependency. Tokio is
  also a dev-dependency for tests, which does not make the adapter available
  without its feature.
- HTML encoding shares a byte-scanning implementation backed by `memchr`
  between the `fmt::Write` and `io::Write` sinks. Interpolated strings must be
  encoded; `HtmlString` is for already-safe HTML emitted verbatim. Preserve
  UTF-8, the five escaped characters (`&`, `<`, `>`, `"`, `'`), and writer
  error propagation when working here.
- Template syntax, codegen, and model contracts are tracked in
  [#3](https://github.com/davidfowl/razor-rust/issues/3). There is no macro or
  codegen crate in the workspace yet. Layouts
  ([#4](https://github.com/davidfowl/razor-rust/issues/4)), partials
  ([#5](https://github.com/davidfowl/razor-rust/issues/5)), and web integration
  ([#6](https://github.com/davidfowl/razor-rust/issues/6)) are roadmap items,
  not implemented features. Do not turn a proposed ADR or an issue example
  into an accepted public API decision.

## Development and verification

Run commands from the repository root. The workspace uses Rust 2021 and
declares **Rust 1.75** as its minimum supported Rust version (MSRV).
Development and CI use stable; changes must also remain compatible with
the declared MSRV.

The stable CI commands are:

```shell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --workspace --all-targets
cargo build --workspace --all-targets --all-features
cargo test --workspace
cargo test --workspace --all-features
cargo run -p razor-slices --example hello
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items --all-features
```

The all-features test command also executes the Tokio adapter doctest.
CI reads MSRV from the workspace manifest and runs build/test in both
feature configurations; with Rust 1.75 installed, reproduce it with:

```shell
cargo +1.75 build --workspace --all-targets
cargo +1.75 build --workspace --all-targets --all-features
cargo +1.75 test --workspace
cargo +1.75 test --workspace --all-features
```

Use checks proportionate to the change, retaining coverage for affected
features and MSRV. For contributor/README-only edits, verify paths and claims
against source and workflows; do not build just to validate prose. For Rust
documentation changes, use the deny-warning rustdoc commands above and run
affected doctests. No standalone Markdown checker is configured today.
For encoder performance work, the existing local benchmark is
`cargo run -p razor-slices --example html_encode_bench --release`; timing is
machine-specific, not a portable performance guarantee.

## Contribution and documentation conventions

- Keep work bounded to an existing roadmap epic, using a focused linked
  issue with a live acceptance checklist. Post a short approach (1-5 bullets)
  before implementation and open an early Draft PR targeting main. Follow
  a PR template if one is present; mark Ready after proportional verification
  and read back the pushed head, base, and issue/PR state.
- Keep the README's current demo, implemented features, next steps, and live
  links accurate as changes land. Describe what works in source, not what an
  unmerged PR or proposal intends to deliver. Update directly related docs
  with the implementation.
- Record tightly related unfinished work as linked **Known gaps / follow-ups**
  in the issue/PR rather than implying the whole epic is complete.
- For genuine hard-to-reverse choices (dependencies, public API shape,
  codegen mechanism), post options and a recommendation on the relevant
  issue for `@davidfowl` / `@DamianEdwards` input. Use the existing
  [ADR convention and template](docs/adr/README.md), adding the corresponding
  numbered ADR in the same PR or a prompt follow-up. Routine bug fixes and
  straightforward implementation details do not need an ADR. Preserve the
  distinction between Proposed and Accepted decisions.
- CI runs on PRs to main and pushes to main. Releases are separately gated
  by manual dispatch; neither a green PR nor documentation work authorizes
  publishing. Review, merge, and release remain maintainer decisions.
