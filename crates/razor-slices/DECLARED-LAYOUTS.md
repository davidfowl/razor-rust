# Typed declared layouts and bare child rendering

Focused synchronous delivery for [#36](https://github.com/davidfowl/razor-rust/issues/36),
a native child of layout epic #4. The original delivery stacked on
PR #46 -> #35 -> #33 -> #31; these deliveries were consolidated into main in #75.
The additive API choice was recorded before implementation in #36; it is not
claimed maintainer-approved generated declaration syntax.

Run from the repository root:

```sh
cargo run -p razor-slices --example declared_layout
```

Exact stdout, checked locally and in CI against `tests/fixtures/declared-layout.html`:

```html
<!DOCTYPE html>
<html><head><title>Greetings &lt;today&gt;</title></head>
<body><main><p>Hello, David &amp; friends!</p></main><footer>After the body</footer></body></html>

Bare partial:
<p>Hello, David &amp; friends!</p>
```

`examples/support/declared_layout.rs` defines `Model`, a typed borrowed
`greeting(&Model) -> io::Result<impl Slice + '_>` factory, a `Page` layout and
a lazy footer section. The entry point renders the same slice first as a full
document, then as a partial:

```rust
let model = page::sample_model();
let document = page::greeting(&model)?;
let mut writer = io::stdout().lock();
document.render_to_writer(&mut writer)?;
let bare = html! { "\nBare partial:\n" {partial(&document)} "\n" };
bare.render_to_writer(&mut writer)
```

## Contract

`declare_layout(body, layout) -> DeclaredLayout<S, L>` owns typed association
metadata, with ordinary Rust references allowed for either argument. It is
distinct from `with_layout`, which explicitly composes a document. Construction
executes nothing; rendering is reusable and lazy. Models are neither copied
nor erased: factories and Rust lifetime checking reject incompatible models
and escaping borrows.

Full `render`, `render_to_writer`, `render_to_string` and `render_to_bytes`
apply the declared layout. Object-safe `Slice::render_body_to_writer` bypasses
declarations and defaults to ordinary byte rendering for existing slices.
`partial`, layout body insertion and associated section content call this
bare-body seam directly. References, boxes and section wrappers forward it.
Layout implementations receive a bare body view, so even direct
`body.render_to_writer` or text/convenience rendering cannot accidentally
re-enter a declared layout.

Explicit composition remains intact: `partial(with_layout(body, layout))`
includes its explicit layout. If its body has a declaration, that declaration
is bypassed at the layout-body insertion point. A declared body containing an
explicit composition retains that composition when used as a partial.
Wrapping a declared body with another declaration replaces its layout for
that rendering; it does not nest both body declarations.

For genuinely nested layouts, associate the inner **layout** with an outer
layout: `declare_layout(body, declare_layout(inner_layout, outer_layout))`.
`DeclaredLayout<S, L>` implements `Layout` when `S: Layout`, rendering outer
then inner then body and unwinding tails. Partials of the resulting document
execute only the body. Layouts control insertion count; this does not enforce
exactly-once rendering.

Sections attached before or after a body declaration remain accessible.
Section insertion is lazy, repeats on each request, rejects empty names and
writes nothing for unknown nonempty names. Declared layouts on associated
section content are bypassed. An outer declared layout sees its immediate inner
layout's own section provider. A custom `Layout::render_layout_section` receives
that provider's original bare body, even after body execution, and can explicitly
forward to its sections. There is no automatic recursive name lookup.

Byte/value/destination errors retain their original I/O kind and source.
Early layout failure skips the body, and body failure skips all later tails.
Output may be partial; there is no buffering, cache, fallback, or implicit
flush. Text methods retain the existing `fmt::Error`/`RenderError` boundary.
Reference/box convenience overrides are forwarded unchanged. The existing
buffered async bridge renders the full document; this is not incremental async
layout support.

## Upstream grounding and known gaps

Pinned reference:
[`DamianEdwards/RazorSlices@552937e5f2f21ebcdc73939be014f3d217ffbb22`](https://github.com/DamianEdwards/RazorSlices/tree/552937e5f2f21ebcdc73939be014f3d217ffbb22).
`RazorSlice.cs`'s `RenderViaLayout` creates a typed declared layout, assigns its
content, and renders that layout with normal layout discovery. Both
`RazorLayoutSlice` variants insert their body through `RenderChildSliceAsync`.
`RazorSlice.Partials.cs` explicitly uses `renderLayout: false` for child text
and pipe writes. `IUsesLayout` supports layouts with or without models.
The executable `_LoremLayout.cshtml` sample declares `_Layout` and inserts
its own body, grounding nested full-layout recursion rather than stacking
declared body wrappers.

This delivery uses typed Rust association values and factories, not C# proxy
creation, generated model declarations or a new DSL. Generated declaration
integration remains in [#3](https://github.com/davidfowl/razor-rust/issues/3).
Synchronous immediate-content section context has
[bounded C# differential coverage](../../docs/parity.md) for #66.
Async layouts/sections are documented in [async-layouts.md](../../docs/async-layouts.md).
Broader cancellation, initialization/disposal and lifecycle parity remain in
[#47](https://github.com/davidfowl/razor-rust/issues/47) and
[#11](https://github.com/davidfowl/razor-rust/issues/11).
`with_section` retains either a `Slice` or `Layout` contract, allowing lazy
sections on an inner layout without erasing its layout behavior.
No Razor `@section` grammar is added. Encoding uses the existing Rust encoder,
not byte-for-byte configurable .NET `HtmlEncoder` parity.
Neither #4 nor the whole port is complete, and delivery is not merge approval.
