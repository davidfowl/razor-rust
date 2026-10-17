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
| [crates/razor-slices/](crates/razor-slices/) | Rendering runtime, model factories and executable template samples |
| [crates/razor-slices-macros/](crates/razor-slices-macros/) | Inline template parser and code generation |
| [crates/razor-slices-axum/](crates/razor-slices-axum/) | Buffered and bounded streaming HTTP responses and real HTTP tests |
| [src/lib.rs](crates/razor-slices/src/lib.rs) | Public runtime exports |
| [src/slice.rs](crates/razor-slices/src/slice.rs) | Synchronous `Slice`, render errors, reference/box implementations, and unit tests |
| [src/html.rs](crates/razor-slices/src/html.rs) | Shared HTML encoding, `HtmlString`, `WriteHtmlValue`, and unit tests |
| [src/async_slice.rs](crates/razor-slices/src/async_slice.rs) | Async writer trait, buffered rendering bridge, optional Tokio adapter, and tests |
| [src/async_slice/context.rs](crates/razor-slices/src/async_slice/context.rs) | Typed request data, runtime-agnostic token and observable cancellation |
| [src/async_slice/value.rs](crates/razor-slices/src/async_slice/value.rs) | Explicit async interpolation contract, strings, booleans and trusted HTML; numeric fast paths share `html.rs` buffers |
| [src/attribute.rs](crates/razor-slices/src/attribute.rs) | Shared validated conditional attributes with explicit sync/async value contracts |
| [examples/async_values.rs](crates/razor-slices/examples/async_values.rs) | Generated mixed-value model, reusable prefix-before-await handshake and exact stdout |
| [examples/combined_async.rs](crates/razor-slices/examples/combined_async.rs) | Combined borrowed/owned generated values, nested layouts, held section and exact document/partial output |
| [examples/combined_server.rs](crates/razor-slices-axum/examples/combined_server.rs) | Same combined declaration over real local HTTP, with separate release and partial routes |
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
  bridge, not incremental mid-template streaming. The separate `AsyncSlice`,
  `AsyncHtmlWriter` and `render_incremental` path writes incrementally and
  supports mid-template flush, lazy async sections and typed declared/nested
  async layouts with immediate-content section context. Async child/section
  execution bypasses declared layouts, but retains explicit composition.
  Generated children, `AsyncHtmlWriter::write_slice`, supplied layout bodies and
  completed layouts check the inclusive 16,384 accepted-output-byte threshold.
  Writes/arbitrary awaits/plain sections do not auto-flush. Root final flush
  and explicit flush remain unchanged; request scope #68/#69 is not assembled.
- `AsyncSliceWriter` is runtime-agnostic. The default-off `tokio` feature adds
  `TokioWriter`; the core has no mandatory async-runtime dependency. Tokio is
  also a dev-dependency for tests, which does not make the adapter available
  without its feature.
- HTML encoding shares a byte-scanning implementation backed by `memchr`
  between the `fmt::Write` and `io::Write` sinks. Interpolated strings must be
  encoded; `HtmlString` is for already-safe HTML emitted verbatim. Preserve
  UTF-8, the five escaped characters (`&`, `<`, `>`, `"`, `'`), and writer
  error propagation when working here.
- Inline `html!` generates reusable slices with nested conditionals/loops.
  `async_template!` generates model-typed reusable `AsyncSlice` declarations
  with explicit await/flush, child and section directives. Typed async
  interpolation supports the existing bool/numeric families, strings/references,
  trusted `HtmlString` and conditional attribute adapters. Custom values require
  `AsyncWriteHtmlValue` (attributes also require `AsyncAttributeValue`); never
  buffer arbitrary synchronous values or infer safety from `Display`.
  Partials, conditional attributes, layouts and named sections are implemented
  in this source. Declared layouts bypass full-document rendering when used
  as children, but explicitly composed documents remain composed. Preserve
  body/section dispatch and all convenience overrides through references
  and boxes; avoid fixing one path while dropping another.
- Axum has distinct buffered/streaming contracts. Buffered failures return
  clean 500 responses; streaming failures propagate body errors after selected
  headers and cannot replace already-started responses. The body owns its
  renderer and bounded frames, with no spawned producer.
  Dedicated cancellation tokens are signalled on unfinished body drop; explicit
  cancellation is a body error. Core request data remains explicit typed model
  input, not automatic framework/service injection. See docs/request-context.md.
- This integration preview assembles unmerged deliveries, not a released
  full port. Broader generated value/grammar parity remains tracked in
  [#3](https://github.com/davidfowl/razor-rust/issues/3); synchronous nested
  section context remains #66 and broader layout/request lifetime parity
  remains #47/#52/#11. Typed async context is documented in docs/async-layouts.md.
  See [docs/integration.md](docs/integration.md) for exact source dependencies.
  Do not turn a proposed ADR or issue example into an accepted public API
  decision. Later owner-delegated reversible implementation choices do not
  imply explicit maintainer approval of every API.

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
