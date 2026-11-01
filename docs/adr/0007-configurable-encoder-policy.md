# 0007. Configurable HTML encoder policy

## Status

Proposed (recommendation pending input from @davidfowl / @DamianEdwards; originating issue [#98](https://github.com/davidfowl/razor-rust/issues/98))

## Context

RazorSlices encodes through a configurable .NET `HtmlEncoder`. Rust has a fixed
five-character encoder (`& < > " '`, ADR [0003](0003-byte-oriented-zero-allocation-html-encoding.md),
[0005](0005-simd-html-encoding.md)). ADR [0006](0006-primitive-formatting-compatibility.md)
defers single-quote and non-ASCII reference behavior to this decision. This is a
new public API surface and hard to reverse.

## Decision

Recommended (implemented as an additive, opt-in API; defaults unchanged):

- `trait HtmlEncoderPolicy { fn escape(&self, char) -> Escape; }` where
  `Escape` is `Keep`, `Entity(&'static str)` or `NumericReference` (`&#xHEX;`,
  upper-case, one reference per scalar).
- `DefaultHtmlEncoder` (the existing five-character behavior; it delegates to
  the memchr path, so `encode_html_to*` and all templates are untouched) and
  `UnicodeHtmlEncoder` (five characters plus non-ASCII and non-whitespace ASCII
  controls as numeric references, producing ASCII-only output).
- `Encoded::new(&str, policy)` implements `WriteHtmlValue` for explicit use.

Alternatives: (a) a concrete struct with allow-list ranges like `UnicodeRanges`
(less extensible, more surface); (b) a global/template-level policy via the
macros or Axum (larger, riskier change; deliberately deferred, macros and the
Axum crate are not touched); (c) an immediate switch to .NET's output
(`&#x27;`, `+` escaped), which changes default bytes.

Out of scope and unresolved: numeric/date/culture formatting (Rust `std` has no
`DateTime` or culture support; recommend leaving it to caller-side formatting
wrapped in `HtmlString`/`Encoded`), and attribute-specific policy. No claim of
byte-for-byte .NET parity: `UnicodeHtmlEncoder` keeps `&#39;` and does not
escape `+`.

## Consequences

Default output and default-path performance are unchanged; the
`html_encode_bench` example asserts the default policy stays within 1.5x of the
free function (observed ~1.02-1.03x). Non-default policies use a scalar `char`
scan (slower; reported in the benchmark). Wiring a policy into templates is
follow-up work needing a decision on the macro/Axum surface.
