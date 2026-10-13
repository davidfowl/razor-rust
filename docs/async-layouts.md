# Typed incremental async layouts

Bounded [#64](https://github.com/davidfowl/razor-rust/issues/64) /
[PR #65](https://github.com/davidfowl/razor-rust/pull/65) under
[#47](https://github.com/davidfowl/razor-rust/issues/47), #4/#11. This source
is based on frozen, Ready/unmerged integration #61 at
`7b6a8aeac067a06f64eae656a79ccdea5b035619`; it is not main or a released port.
Independent async value work #62/#63 is not included.

## Typed association and content context

`AsyncLayout: Sync` renders around a generic `B: AsyncSlice`, using the existing
`AsyncHtmlWriter` and `Send` render futures. Layouts can be handwritten or delegate
to generated generic `async_template!` views. A typed Rust factory associates
the page and borrowed layout models:

```rust,ignore
let body = with_async_section(PageBody::new(model.body), "header", Header::new(model))?;
let inner = with_async_section(Inner(model.title), "footer-links", Text::new("<inner links>"))?;
let page = declare_async_layout(body, declare_async_layout(inner, Outer(model.title)));
render_incremental(&page, &mut writer).await?;
```

See the [complete generated-model source](../crates/razor-slices/examples/support/async_layout.rs).
There is no new macro declaration grammar or Razor `@section` directive.
These Rust factory/trait names are delegated reversible implementation choices,
not claimed explicit maintainer syntax acceptance.

`declare_async_layout` owns the arguments (which may be references). Full
`render_into` applies the declared layout chain. Bare `render_body_into`,
generated `@render { child };`, layout body insertion and section content
bypass declarations. `with_async_layout` is explicit document composition:
it remains composed when inserted as a child or section. Reference and box
wrappers forward full/body/section dispatch without erasing overrides.
`with_async_section` now returns a concrete `AsyncSectioned<S, T>` retaining
either the body's `AsyncSlice` or `AsyncLayout` contract.

Each outer layout sees its **immediate content provider**. In the example:
the outer layout's `footer-links` executes the inner layout's section; the inner
layout's `header` executes the page's section. Unknown names do not automatically
fall back through the layout chain. Names remain nonempty, case-sensitive
optional lookup keys, with outermost explicit associations winning duplicates.
Section content executes freshly at every insertion, not at construction.

`AsyncLayout::render_layout_section_into(body, name, output)` lets a custom
layout's own section use that layout's original child context, including after
body execution or repeated section insertion. Explicit forwarding can call
`output.write_section(body, child_name).await`; it is not automatic global
inheritance. The context is immutable and render-local, not shared ambient state.
An association outside an already-declared page supplies sections to that page's
consumers; it does not rebind the inner declaration's original body.

## Observable exact output

```sh
cargo run --locked -p razor-slices --example async_layout --features tokio > target/async-layout-output.txt
diff -u crates/razor-slices/tests/fixtures/async_layout.txt target/async-layout-output.txt
```

The [fixture](../crates/razor-slices/tests/fixtures/async_layout.txt) is:

```text
prefix-before-section-await: <html><title>&lt;Title&gt;</title><main><h1>&lt;Title&gt;</h1><header>
prefix-before-section-await: <html><title>&lt;Title&gt;</title><main><h1>&lt;Title&gt;</h1><header>
document: <html><title>&lt;Title&gt;</title><main><h1>&lt;Title&gt;</h1><header>&lt;Title&gt;</header><p>&lt;body&gt; &amp; 雪</p></main><footer>&lt;inner links&gt;</footer></html>
partial: <p>&lt;body&gt; &amp; 雪</p>
section-renders: 2
```

The [executable](../crates/razor-slices/examples/async_layout.rs) renders the same
borrowed page twice through a **one-byte duplex buffer**. The reader compares
the entire prefix and the unchanged section-completion count before notifying
later section work. The generated header explicitly flushes before awaiting.
It then compares every completed byte. The generated bare partial emits only
the page body and does not execute its sections. No sleep or producer task is
used; full-page/section buffering would deadlock this handshake.

[Regressions](../crates/razor-slices/tests/async_layouts.rs) additionally hold a
flush pending even after application release, stop at all 15 writer boundaries,
retain original destination error sources at every write and the mid-template
flush, and drop pending section/write futures without suffix or final flush.
They cover three nested layout levels, section-owned child context after body,
repeated/optional/shadowed names, before/after associations, explicit composition,
borrowed repeated output and generated/wrapper dispatch. Compiler-negative
doctests reject non-layouts, synchronous bodies, escaped model lifetimes and
non-Send layout awaits. Existing synchronous and incremental suites stay intact.

Every write/flush is awaited on the caller's original destination. No new buffer,
channel, executor or dependency is introduced; only the existing encoder's fixed
scratch remains. Only `render_incremental` final-flushes after successful full
execution. Errors stop immediately, preserving variants, I/O kinds and sources;
accepted bytes are not rolled back. Dropping the render future drops in-flight
awaits and stops polling, but cannot cancel separately spawned work or undo
destination-accepted prefixes. The writer stays caller-owned.

## Executable upstream grounding and known gaps

At pinned upstream
[552937e5f2f21ebcdc73939be014f3d217ffbb22](https://github.com/DamianEdwards/RazorSlices/tree/552937e5f2f21ebcdc73939be014f3d217ffbb22):

- [`RazorSlice.RenderViaLayout`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.cs)
  assigns the content slice before recursively rendering the declared layout.
- [`RenderChildSliceAsync`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.Partials.cs)
  renders the child with `renderLayout: false`.
- [`RazorLayoutSlice`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorLayoutSlice.cs)
  executes body/sections on immediate `ContentSlice`; the base section method
  produces no output for unknown names. No required-section exception is
  inferred from contradictory XML documentation.
- [`_LoremLayout`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/samples/WebApp/Slices/Lorem/_LoremLayout.cshtml)
  reads its page's `lorem-header`, declares `_Layout`, and supplies its own
  `footer-links` to that outer layout.

**Known gaps / follow-ups:** synchronous nested provider context is separately
tracked in [#66](https://github.com/davidfowl/razor-rust/issues/66); it is not
silently fixed or claimed here. #47/#11/#52 retain initialization/disposal,
request tokens/context, completion flags and automatic-flush/encoder-policy
parity. #3 retains broader grammar/value parity; #62/#63 remains independently
delivered. Async trait objects/local-only futures are not added. Fresh Rust
renders re-execute borrowed templates rather than reproducing C# pooling or
disposal. No whole-epic, whole-port, merge, release or deployment claim.
