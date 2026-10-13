# First inline templates

Run from the repository root:

```sh
cargo run -p razor-slices --example inline
```

Output:

```html
<p>Hello, Damian &amp; David!</p>
<p>Hello, Damian &amp; David!</p>
<div title="&quot;&lt;&amp;&#39;">Hello, 世界 42 true <strong>trusted HTML</strong></div>
```

```rust
use razor_slices::{html, Slice};

fn greeting(name: &str) -> impl Slice + '_ {
    html! { "<p>Hello, " {name} "!</p>" }
}
```

## Initial contract

`html!` returns `InlineSlice<F>`, which implements the existing synchronous
`Slice` trait. Its grammar is zero or more string literals or `{ expression }`
parts, with no commas or implicit separators. Recursive `if`/`else` and `for`
template bodies are documented in [CONTROL_FLOW.md](CONTROL_FLOW.md).
Raw string literals work too.
Literal content, including spaces, newlines and braces, is preserved exactly.
Outside literals, whitespace only separates Rust tokens. Bare HTML is rejected.
The macro does not validate HTML structure or infer attribute contexts.

Each expression must implement `WriteHtmlValue`. Strings escape `& < > " '`;
numbers and booleans use the existing primitive formatting paths. `HtmlString`
is an explicit trusted-HTML opt-out, never an automatic fallback for unsupported
types. Do not wrap untrusted input in `HtmlString`. HTML escaping is not URL,
JavaScript or CSS sanitization.

The renderer captures by `move`, using ordinary Rust ownership rules. It borrows
each expression's result during the write rather than consuming it. Expressions
are evaluated in order on **each render**, not at template creation. Captured
owned strings survive the creating scope; captured references cannot outlive
their values. `Fn` rules reject moving out of or mutably borrowing captures;
interior mutability is still ordinary Rust. To keep ownership outside a template,
bind a reference first. Type checking and diagnostics come from Rust/syn; no
untyped model or implicit `Display` conversion is introduced.

`Slice::render`, `render_to_string`, `render_to_bytes`, and the existing buffered
`render_async` bridge all work. `InlineSlice::write_to` writes incrementally to
`io::Write`, retains the original `io::Error`, and does not flush the destination.
The `fmt::Write` bridge decodes UTF-8 incrementally, including split code points,
without buffering the whole output. Invalid/incomplete UTF-8 and value/writer
failures become `fmt::Error` on `Slice::render`, as required by that existing
interface; convenience rendering wraps it in `RenderError`. Direct byte output
does not validate custom value bytes. Writes stop at the first error; a failed
render may leave partial output, but the template remains reusable.

The runtime's declarative entry point supplies a hygienic crate path to the
proc macro, so renamed Cargo dependencies work without crate-name discovery.
The standalone fixture at `tests/renamed/Cargo.toml` exercises this on stable
and MSRV CI. The original `hello` example now uses the macro with unchanged
output; the `inline` example additionally demonstrates reuse, Unicode, primitive
values and trusted raw HTML. The gated release workflow selects the workspace
so Cargo can package/verify the macro dependency before the runtime; no release
is part of this milestone.

## Relationship to Razor Slices and decision provenance

The owner accepted the inline proc-macro mechanism in
[#3](https://github.com/davidfowl/razor-rust/issues/3#issuecomment-6132090066).
Under the subsequent delegated "Pick one" choice, the coordinator selected a
reusable synchronous `Slice`, not an eagerly rendered `HtmlString`. Quoted literal
parts are a small reversible initial syntax choice that preserves markup
whitespace without reconstructing it from Rust token spans. **This concrete
syntax/capture contract is an implementation choice, not a claim of explicit
maintainer syntax approval.**

Upstream's
[`RazorSlice.Write.cs`](https://github.com/DamianEdwards/RazorSlices/blob/main/src/RazorSlices/RazorSlice.Write.cs)
distinguishes `WriteLiteral` from encoded `Write(string)` and raw
`Write(HtmlString)`/`IHtmlContent`; this macro preserves that direction using the
Rust runtime's existing encoding policy. It does not claim byte-for-byte parity
with .NET's configurable `HtmlEncoder` (for example Unicode and entity spellings).
Upstream's
[`RazorSlice.cs`](https://github.com/DamianEdwards/RazorSlices/blob/main/src/RazorSlices/RazorSlice.cs)
disposes render instances. Reusability here follows Rust's existing `Slice::render`
`&self` contract, not a claim that the upstream lifecycle is identical.

## Known gaps

This is the bounded first milestone in
[#30](https://github.com/davidfowl/razor-rust/issues/30), not the whole port.
Template declarations and async expressions/codegen remain in
[#3](https://github.com/davidfowl/razor-rust/issues/3).
Ordinary Rust expressions can contain blocks or conditionals but there are no
Razor directives. Layout sections/declarative association remain
[#4](https://github.com/davidfowl/razor-rust/issues/4); an explicit synchronous
layout/body sample is documented in [LAYOUTS.md](LAYOUTS.md).
Async/context-aware partials remain [#5](https://github.com/davidfowl/razor-rust/issues/5), web integration
[#6](https://github.com/davidfowl/razor-rust/issues/6), and true asynchronous
mid-template streaming [#11](https://github.com/davidfowl/razor-rust/issues/11).
Explicit synchronous partial composition and a typed model-list sample are now
documented in [PARTIALS.md](PARTIALS.md). Use `partial(child)` to render a child
inside `html!`; there is still no implicit slice interpolation.

The shared ADR/index/README correction is owned by
[#28](https://github.com/davidfowl/razor-rust/issues/28)/[PR #29](https://github.com/davidfowl/razor-rust/pull/29).
When this implementation lands, those surfaces should link this document/sample,
show the new macro crate, and replace "no codegen" with this limited contract.
