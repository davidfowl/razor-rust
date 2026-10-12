# 0005. SIMD-vectorized HTML encoder byte scanning

## Status

Accepted (implemented).

## Context

[#13](https://github.com/davidfowl/razor-rust/pull/13) (tracked by ADR
[0003](./0003-byte-oriented-zero-allocation-html-encoding.md)) landed a
byte-oriented HTML encoder that scans a `&str`'s UTF-8 bytes one at a time
looking for the 5 escaped ASCII bytes (`& < > " '`), emitting each run of
safe bytes as a single `write_all`/`write_str` call.
[DamianEdwards requested][simd-comment] that this byte-seeking (and the
equivalent encoding/decoding operations) be vectorized with SIMD, matching
upstream's C# `HtmlEncoder` implementation.

This is a genuine architecture tradeoff, not routine implementation detail:
it has direct consequences for dependency surface, MSRV (the workspace
targets `rust-version = "1.75"` and CI (`ci.yml`) builds on `stable` plus a
pinned MSRV toolchain — no nightly job exists), and the amount of `unsafe`
code the project would own and have to review.

### Options considered

1. **Nightly-only `std::simd` (`portable_simd`)** — closest to native SIMD
   with zero new dependencies, but nightly-gated with no stabilization
   timeline. Ruled out: CI has no nightly toolchain, and adopting it here
   would require either a new nightly CI job just for this module or
   silently breaking builds on stable/MSRV.
2. **Stable portable-SIMD arithmetic crate (e.g. `wide`)** — fits numeric
   lane computation well, but our bottleneck is "find the next occurrence of
   one of 5 bytes in a byte slice," not arithmetic. Using `wide` directly
   would mean hand-writing the byte-comparison/mask/position-extraction
   logic ourselves — most of the complexity (and bug surface) of option 3
   without a purpose-built API.
3. **Hand-rolled `unsafe` platform intrinsics** (SSE2/AVX2 on x86_64, NEON on
   aarch64, scalar fallback elsewhere) with runtime feature detection —
   maximum control and performance, but adds `unsafe` code per target that
   the project would have to review and maintain, and multiplies the test
   matrix (every intrinsic path needs coverage on real hardware/CI runners).
4. **Adopt the [`memchr`](https://crates.io/crates/memchr) crate** for the
   byte-run scan — it already implements runtime-dispatched SIMD (SSE2/AVX2
   on x86_64, NEON on aarch64, scalar elsewhere), supports Rust versions far
   below our 1.75 MSRV, and is one of the most widely used and fuzzed
   low-level crates in the ecosystem (a dependency of `regex`, `ripgrep`,
   and, transitively, much of the ecosystem already). It exposes needles of
   up to 3 distinct bytes (`memchr`, `memchr2`, `memchr3`); we need 5, so the
   scan decomposes into two sub-scans — `memchr3(b'&', b'<', b'>')` and
   `memchr2(b'"', b'\'')` — taking whichever match is nearer.
5. **Keep the current scalar per-byte scan** — zero new risk, but does not
   address the request and leaves a real (if modest for small, typical
   HTML-fragment-sized payloads) performance gap versus upstream on larger
   inputs.

## Decision

Adopt option 4: use the `memchr` crate's `memchr2`/`memchr3` to jump directly
to the next escaped byte, instead of testing every byte in a loop. This
satisfies the vectorization request while keeping the project on stable
Rust within MSRV 1.75 (disqualifying option 1), and keeps the `unsafe`/
per-target-intrinsic maintenance burden in an externally maintained,
heavily audited dependency rather than in this repository (avoiding option
3's review cost). `memchr` has no dependencies of its own; the benefit
shows up on payloads with long runs of unescaped text (see benchmark
results below), which is exactly where the unvectorized scan was weakest
and where typical HTML templates spend most of their bytes (static markup,
with escapes concentrated at a minority of interpolation points).

`write_encoded` (shared by both `encode_html_to` and `encode_html_to_writer`)
now uses `next_escape` (built on `memchr2`/`memchr3`) to find the next
escaped byte's index directly, instead of iterating byte-by-byte. The public
API and encoded output are unchanged; this is purely an internal
scanning-strategy swap, verified by the existing encoder test suite (plus
new cases for consecutive escapes, escapes at the start/end of input, and
long escape-free runs) and a manual `std::time::Instant`-based comparison
(`examples/html_encode_bench.rs`) across escape-heavy vs. escape-free
payloads of varying sizes. A `criterion`-based benchmark was deliberately
avoided: its dependency tree pulls in a `clap_builder` version requiring
Cargo's unstable `edition2024` feature, which breaks the `msrv` CI job's
`cargo build --workspace --all-targets` step on the pinned 1.75 toolchain.

`cargo run -p razor-slices --example html_encode_bench --release` (x86_64,
AVX2) shows the new scan is **17.8x faster** on an 8 KB payload with no
escapes, and **3.3x faster** on an 8 KB payload with sparse escapes (one
every 512 bytes — closer to typical template content), but **regresses**
on pathologically escape-dense payloads (one escape every 8 bytes: ~6x
*slower*, and a small ~32-byte payload with an escape every 8 bytes is
~1.5x slower). This is expected: each `next_escape` call does two `memchr`
sub-scans, and that fixed per-call overhead dominates when runs between
escapes are too short for the SIMD skip-ahead to pay for itself. Real HTML
content (templates are mostly static markup, with escapes concentrated at
a minority of interpolated-value boundaries) looks like the sparse/no-
escape cases, not the dense one, so this is judged an acceptable tradeoff —
flagged here and in the PR's "known gaps" as a follow-up if a pathological
workload is ever reported in practice (e.g. a hybrid that falls back to the
scalar scan below some run-length threshold).

## Consequences

- New dependency: `memchr` (no transitive dependencies of its own), added to
  `razor-slices`'s `[dependencies]`.
- The HTML encoder's byte-run scan is now SIMD-accelerated (via `memchr`'s
  runtime CPU feature detection) on x86_64 and aarch64, with an efficient
  scalar fallback elsewhere — no platform-specific `unsafe` code lives in
  this repository.
- No public API change; `encode_html_to`/`encode_html_to_writer`/
  `WriteHtmlValue` behave identically, just with a faster internal scan for
  larger payloads.
- If a future need arises for `no_std` support (tracked as a gap in ADR
  0003, revisit per [#6](https://github.com/davidfowl/razor-rust/issues/6)),
  `memchr` also ships a `core`-only build via its `alloc`-optional, `std`-
  optional feature flags, so this does not foreclose that option.

[simd-comment]: https://github.com/davidfowl/razor-rust/issues/2#issuecomment-6131226727
