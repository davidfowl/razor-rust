# Lazy sections on incremental async output

Focused milestone [#48](https://github.com/davidfowl/razor-rust/issues/48)
under [#47](https://github.com/davidfowl/razor-rust/issues/47), delivered in
[PR #49](https://github.com/davidfowl/razor-rust/pull/49).
This branch explicitly depends on **Ready/unmerged PR #38**
(`64b626a4869d7b7a46a1563e52f180ded929c458`). Neither that branch nor the
synchronous layout/section stack #35/#46 is changed. Integrate #38 first;
these APIs are not yet on main.

## Observe a section before its awaited work completes

```sh
cargo run -p razor-slices --example async_sections --features tokio
```

Actual stdout, compared to `crates/razor-slices/tests/fixtures/async_sections.txt`
locally and in Actions:

```text
Before releasing section await: <page><header>Loading...</header>
After releasing section await:
<h1>&lt;雪 &amp; ready&gt;</h1>
<main>Body</main>
</page>
```

The page inserts a `header` section, then its body, then an absent optional
scripts section. The header writes and flushes loading markup, awaits a
`Notify`, then writes its escaped model title. A **one-byte duplex buffer**
forces partial writes. The consumer must receive/assert the complete prefix
before releasing the header's await; no body/tail can be written until the
header completes. Buffering the whole section/page would deadlock this
handshake. No sleeps, detached tasks or timing-based assumptions are used.

## Contract

`with_async_section(body, name, content) -> io::Result<impl AsyncSlice>` owns
the two async slices (or their references) and copies the name. Construction
does not execute either renderer. Ordinary `render_into` delegates unchanged
to the body. Section dispatch executes only a matching section's body;
otherwise it delegates to the wrapped body's sections. Chain associations
for multiple names. Repeated names use the outermost association, without
executing shadowed content.

`AsyncSlice::render_section_into(name, output)` is the direct, statically
dispatched provider method, defaulting to optional no-output execution.
`AsyncHtmlWriter::write_section(&provider, name).await` validates the name
before calling even handwritten providers. The example's page uses:

```rust,ignore
output.write_literal("<page>").await?;
output.write_section(&self.body, "header").await?;
self.body.render_into(output).await?;
output.write_section(&self.body, "optional-scripts").await?;
output.write_literal("</page>\n").await?;
```

This is an excerpt showing execution order, not generated macro syntax.
See `crates/razor-slices/examples/async_sections.rs` for a complete runnable
page and section implementation; the API doctest in `with_async_section`
demonstrates typed model association.

Names are nonempty, case-sensitive string lookup keys. Whitespace and Unicode
names are allowed; names are not emitted markup and are not trimmed or
normalized. Empty association names fail at construction with `InvalidInput`.
Empty rendering requests fail with that I/O error inside
`AsyncRenderError::Write`, without touching the destination. Custom direct
providers must also reject empty names. Unknown nonempty names write/flush
nothing by default, even if the destination would fail a write.

Static `&S` and `Box<S>` providers forward ordinary and section execution;
this does not make `AsyncSlice` object-safe. Model/reference lifetimes follow
Rust borrowing; association names may be dropped after construction.
The existing `AsyncSlice: Sync` and writer `Send` bounds remain in force and
returned futures are `Send`. A synchronous `Slice` is not an async section.
The synchronous `with_section`/`section` API remains separate in #46.

Each insertion executes its section again, lazily in source order. There is
no implicit final section flush, cache, required-section mode, fallback body,
model reset or eager section buffer. Sections use the same
`AsyncHtmlWriter`; literal markup stays trusted, `write_value` escapes model
values once with the existing bounded UTF-8 encoder. No new executor,
dependency, channel or unbounded render buffer is introduced.

Every destination write/explicit flush is awaited before later section work.
Failures retain their original `AsyncRenderError` variant and original I/O
kind/source; `?` skips later body/sections/tails. The top-level
`render_incremental` final-flushes only after success, just as before this
delivery. Output may already be visible on failure. A section may explicitly
flush its prefix, but the insertion itself never flushes.

Dropping a pending render/section future stops polling it; it does not
roll back accepted output, flush/complete its body or cancel independently
spawned application work. Do not blindly restart cancelled rendering on the
same destination expecting atomic replacement. Application awaits own their
cancellation behavior; no built-in token or writer shutdown is added.

## Grounding and remaining parity

Pinned upstream
[`552937e5f2f21ebcdc73939be014f3d217ffbb22`](https://github.com/DamianEdwards/RazorSlices/tree/552937e5f2f21ebcdc73939be014f3d217ffbb22):
[`RazorLayoutSlice.RenderSectionAsync`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorLayoutSlice.cs)
rejects empty names and awaits execution on the content slice at insertion;
[`RazorSlice.ExecuteSectionAsync`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.Layout.cs)
defaults to no output. Sample section overrides use case-sensitive checks
and ignore unmatched names. This follows executable behavior, not the
contradictory XML comment describing a missing-section exception.
No Razor `@section` directive is introduced, consistent with upstream's
intentional streaming-compatible manual section overrides.

This is explicit async composition, not a new async layout trait or inherited
ambient context. Generated/declarative nested layout context remains
[#47](https://github.com/davidfowl/razor-rust/issues/47)/
[#36](https://github.com/davidfowl/razor-rust/issues/36).
Generated async template/model syntax remains
[#3](https://github.com/davidfowl/razor-rust/issues/3); HTTP
streaming/cancellation integration remains
[#6](https://github.com/davidfowl/razor-rust/issues/6).
Writer lifetime/token/completion/threshold-flush parity and broader .NET
encoder policy remain [#11](https://github.com/davidfowl/razor-rust/issues/11).
The original buffered synchronous bridge is unchanged. This is not a claim
of full-port/integrated-stack completeness or explicit maintainer API approval:
the bounded Rust API choices were owner-delegated.

See [async-rendering.md](async-rendering.md) for the underlying incremental
execution contract and destination visibility/cancellation limits.
