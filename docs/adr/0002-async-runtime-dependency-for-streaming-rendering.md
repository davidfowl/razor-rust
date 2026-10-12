# 0002. Async runtime dependency for streaming rendering

## Status

Accepted (implemented in [#17](https://github.com/davidfowl/razor-rust/pull/17)).
The deeper tokio-vs-agnostic tradeoff question is still open for maintainer
reply, but the reversible foundational design below has landed.

## Context

Upstream's primary rendering path is async and streaming
(`RenderAsync(PipeWriter, ...)` / `RenderAsync(TextWriter, ...)`), with
mid-template `FlushAsync()` support. Porting this to Rust
([#11](https://github.com/davidfowl/razor-rust/issues/11)) requires choosing
an async runtime dependency for the core `razor-slices` crate — a real
new-dependency/architecture decision, since generated `@await` codegen (#3)
would end up depending on whatever signature is chosen.

Three options were posted on the issue, [tagging @davidfowl / @DamianEdwards][comment]:

- **A. `tokio::io::AsyncWrite`** — best ecosystem fit for axum (the
  framework-integration target in #6, itself tokio-only), but hard-couples
  the core runtime crate to tokio, blocking non-tokio consumers
  (`async-std`, `smol`, WASM).
- **B. Runtime-agnostic via `futures::io::AsyncWrite`** (or the
  `futures-core`/`futures-io` subset) — doesn't lock consumers into tokio and
  has compat shims back to tokio, but adds translation friction for the
  common axum case and `futures-io`'s `AsyncWrite` has seen less maintenance
  momentum than `tokio::io::AsyncWrite`.
- **C. Hand-rolled minimal async write trait** (no dependency) using native
  `async fn` in traits, with a feature-gated `tokio` adapter (off by
  default). Keeps the core crate dependency-light (zero dependencies today)
  while making the tokio path a one-line feature flag for axum consumers.
  Tradeoff: more code to own, and `async fn`-in-traits isn't `dyn`-compatible
  yet (needs boxing for trait-object use, same constraint `async_trait`
  historically worked around).

## Decision

**Recommended and implemented: C** — a hand-rolled `AsyncSliceWriter` trait
(`write_all`/`flush`) with zero mandatory async-runtime dependency, plus a
default-off `tokio` Cargo feature providing a `TokioWriter` adapter. This
keeps `razor-slices` dependency-light for sync-only or non-tokio consumers
while making the tokio path trivial for the axum integration (#6).

This was implemented in PR #17 as the reversible, foundational part of #11
(`AsyncSliceWriter` trait, `render_async(slice, writer)` bridge function, the
`tokio` feature and `TokioWriter` adapter) specifically *because* it avoids
locking in the harder tokio-vs-runtime-agnostic question — that question is
still open pending a maintainer reply on the [original comment][comment].
`AsyncSliceWriter` was kept as a separate trait (not a new `Slice` method) to
preserve `Slice`'s object-safety (`Box<dyn Slice>` / `&dyn Slice` from #16).

## Consequences

- `razor-slices` gained async/streaming rendering without a mandatory
  runtime dependency; consumers opt into `tokio` via a Cargo feature.
- `dyn AsyncSliceWriter` isn't usable yet (native `async fn`-in-traits
  limitation) — only generic (`impl AsyncSliceWriter`) call sites work for
  now.
- No `async-std`/`smol`/`futures::io::AsyncWrite` adapters exist yet;
  straightforward to add as separate default-off features later.
- Genuinely incremental mid-template streaming (vs. today's
  buffer-then-write bridge) still depends on `@await` support landing in
  codegen (#3).
- CI does not yet exercise `--all-features` (the `tokio` feature) — tracked
  as a known gap under #7.

[comment]: https://github.com/davidfowl/razor-rust/issues/11#issuecomment-6130392034
