# 0003. Byte-oriented, zero-allocation HTML encoding

## Status

Accepted (implemented in [#13](https://github.com/davidfowl/razor-rust/pull/13),
closing [#10](https://github.com/davidfowl/razor-rust/issues/10)).

## Context

The initial core runtime ([#2](https://github.com/davidfowl/razor-rust/issues/2))
landed a synchronous, `String`-based `encode_html_to` (`fmt::Write`) that
HTML-encodes one `char` at a time. Upstream's
`PipeWriterHtmlExtensions.cs` instead writes UTF-8 directly to a
`PipeWriter`, with low-allocation fast paths for primitive values (avoiding
`ToString()`-style boxing). [#10](https://github.com/davidfowl/razor-rust/issues/10)
captured this gap, deliberately deferred out of the MVP core-runtime PR so
that PR could land without blocking on it.

The tradeoff: keep the simple `String`/`fmt::Write`-only encoder (less code,
but re-encodes UTF-8 and allocates a `String` per primitive value via
`to_string()`), versus adding a byte-oriented path that writes directly to
any `std::io::Write` sink and avoids heap allocation for primitives.

## Decision

Add `encode_html_to_writer(value, &mut dyn io::Write)`, which scans the raw
UTF-8 byte slice for the 5 escaped ASCII bytes (`& < > " '`) and emits each
run of safe bytes as a single `write_all` call, instead of decoding/
re-encoding one `char` at a time. The existing `fmt::Write`-based
`encode_html_to` now shares the same byte-scanning core, so existing callers
get fewer/larger writes for free with no API change.

A new `WriteHtmlValue` trait gives `bool`/integers/floats a fast path that
formats into a small stack buffer (`FixedBuf<N>`, no heap allocation)
instead of going through `to_string()`; `str`/`String` HTML-encode via
`encode_html_to_writer`; `HtmlString` writes its already-encoded content
verbatim. `f64`/`f32` fall back to `to_string()` only when a value's
`Display` expansion doesn't fit the stack buffer, to preserve correctness
without ever panicking.

This mirrors upstream's non-boxing primitive write paths and was judged a
genuine (if smaller-scale and non-blocking) performance/architecture
tradeoff worth implementing directly, rather than a decision requiring
maintainer sign-off before proceeding — unlike #0001/#0002, no options were
posted to the issue for @davidfowl/@DamianEdwards input before implementing.

## Consequences

- Core runtime now has a byte-oriented, zero-allocation encoding path in
  addition to the original `fmt::Write` path, with no breaking API change.
- Once template codegen (#3) lands, generated `Slice::render` impls should
  prefer `WriteHtmlValue`/`encode_html_to_writer` over hand-written
  `fmt::Write` calls for interpolated expressions.
- No `no_std`/`bytes::BufMut` support yet — only `std::io::Write` — revisit
  if/when framework integration (#6) needs it.
- [DamianEdwards suggested][simd-comment] the encoding/decoding operations
  should be vectorized with SIMD to match the C# implementation. This is a
  non-owner comment from a project contributor and has not yet been
  acted on or acknowledged — flagged here for maintainer follow-up rather
  than acted on unilaterally.

[simd-comment]: https://github.com/davidfowl/razor-rust/issues/2#issuecomment-6131226727
