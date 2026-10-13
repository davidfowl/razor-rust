# 0002. Async runtime dependency for streaming rendering

## Status

Accepted (implemented in [#17](https://github.com/davidfowl/razor-rust/pull/17)).
The foundation is merged. The additive incremental execution extension below
is a delegated implementation choice, not a historical maintainer approval
of a final async codegen/runtime contract.

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

- `razor-slices` gained a buffered async I/O bridge without a mandatory
  runtime dependency; consumers opt into `tokio` via a Cargo feature.
- `dyn AsyncSliceWriter` isn't usable yet (native `async fn`-in-traits
  limitation) — only generic (`impl AsyncSliceWriter`) call sites work for
  now.
- No `async-std`/`smol`/`futures::io::AsyncWrite` adapters exist yet;
  straightforward to add as separate default-off features later.
- Hand-written incremental execution is now available through the separate
  `AsyncSlice` contract below; generated `@await` support remains under #3.
- CI exercises default features and `--all-features` (including the `tokio`
  adapter doctest) in stable lint/build/test/docs and MSRV build/test checks,
  covering the follow-up tracked in [#20](https://github.com/davidfowl/razor-rust/issues/20)
  under #7.

## Delegated incremental execution extension

[#37](https://github.com/davidfowl/razor-rust/issues/37), under #11, adds
`AsyncSlice`, `AsyncHtmlWriter` and `render_incremental` independently of the
unmerged macro/layout work. This reversible additive API is chosen under
delegated implementation discretion; it does not assert that maintainers
approved the original dependency options or a generated async template
contract. No new production dependency is introduced.

Keep `Slice` and the fully buffered `render_async` bridge unchanged.
Hand-written async slices await borrowed literal writes, bounded escaped
string writes and explicit mid-template flushes in source order. The helper
final-flushes only after success. Reusing the existing synchronous encoder in
1,024-byte UTF-8 chunks bounds scratch space at 6,144 bytes, avoiding duplicated
escape rules or full-page buffering. This costs a bounded copy per escaped
chunk; direct borrowed literal writes do not copy.

Tradeoffs: static generic dispatch, `Sync` slices/`Send` writers and futures,
no token API or rollback on dropped futures, no automatic threshold flushing,
and unconditional final flush after success (unlike upstream's threshold-based
pipe flush). These limitations and the observable prefix-before-await example
are documented in [async rendering](../async-rendering.md). Async macro grammar,
framework response integration and deeper C# cancellation/flush/encoder parity
remain under #3/#6/#11 rather than blocking this independent runtime delivery.

[comment]: https://github.com/davidfowl/razor-rust/issues/11#issuecomment-6130392034
