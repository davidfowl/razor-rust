# Incremental async rendering

`render_async(&slice, &mut writer)` remains the **fully buffered** bridge for
synchronous `Slice`: render the complete `Vec<u8>`, then await one write and
one flush. It does not expose bytes during template execution.

`AsyncSlice` is a separate, reusable, statically dispatched async template
contract. `render_incremental` calls `render_into` with a borrowed
`AsyncHtmlWriter`, then awaits a final flush **only after success**.
Implementations write/await/flush in source order, propagating failures with
`?`; nested async slices can call `render_into` on the same output without
an implicit final flush. This does not change synchronous templates, macro
grammar, layouts, or require an executor in production default features.

## Observable example

```shell
cargo run -p razor-slices --example incremental --features tokio
```

```text
Before releasing later await: <p>Loading...</p>
After releasing later await: <p>&lt;雪 &amp; ready&gt;</p>
```

The example uses a 16-byte Tokio duplex buffer. The template writes and flushes
the prefix, then awaits a `Notify`. The reader must receive and assert the
complete prefix **before** releasing that await. Only then can the template
write the escaped suffix. Buffering the whole page before writing would
deadlock this handshake; this is not a timing/sleep-based streaming claim.
Tokio `sync` is enabled only as a dev dependency for this example; the
production adapter still needs only the optional `io-util` feature.

`AsyncHtmlWriter::write_literal` borrows trusted markup directly; do not pass
untrusted values. `write_value(&str)` reuses the synchronous HTML encoder to
escape `& < > " '`. It processes at most 1,024 UTF-8 input bytes per chunk
using a fixed 6,144-byte scratch buffer, adjusting boundaries so Unicode
characters are never split. There is no full-value/page allocation, channel
or background task. Each write and flush is awaited before later work, so
writer backpressure is honored. Sink acceptance is not network visibility:
flush according to the destination's own contract.

## Named sections

Explicit lazy async sections are described in [async-sections.md](async-sections.md).
`with_async_section` associates reusable section renderers;
`AsyncHtmlWriter::write_section` validates and awaits their execution at each
insertion on the same destination. The `async_sections` example proves that a
section's prefix is observable before its later await completes. This is
separate from inherited/declarative layout context and generated async syntax.

## Errors, repetition and cancellation

Writer/flush errors become `AsyncRenderError::Write`; template formatting
errors use the existing `Render` variant. The caller receives failures
immediately; no final flush, retry or success fallback follows a failure.
Partial output may already be visible and is not rolled back.

Each render executes the template again. Model state, external awaits and
side effects belong to the implementation; no output is cached and no state
is reset. Shared borrowing allows reuse but does not guarantee deterministic
output from stateful templates.

Drop the rendering future to stop polling it. There is no built-in
cancellation token, rollback or writer shutdown. A pending destination write
can have accepted part of its input; independently spawned application work
may continue. Do not restart cancelled rendering on the same destination
expecting an atomic replacement. Application futures own their cancellation
behavior. `AsyncSlice: Sync` and writer `Send` bounds make returned futures
`Send`; generic dispatch does not support `dyn AsyncSlice` or
`dyn AsyncSliceWriter`, and local-only non-`Send` writers are not supported
by this new path.

## Upstream grounding and remaining parity

At upstream commit
[`552937e5f2f21ebcdc73939be014f3d217ffbb22`](https://github.com/DamianEdwards/RazorSlices/tree/552937e5f2f21ebcdc73939be014f3d217ffbb22):

- [`RazorSlice.cs`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.cs)
  executes `ExecuteAsync` against the selected writer; `FlushAsync` awaits
  writer flush with its cancellation token and errors propagate.
- [`Streaming.cshtml`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/samples/WebApp/Slices/Streaming.cshtml)
  flushes a prefix before awaited delays and flushes subsequent updates.
- [`RazorSlice.Write.cs`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.Write.cs)
  distinguishes trusted literal markup from encoded string values.

This is execution/flush parity, not an exact C# API translation. C# literal
writes are synchronous into writer buffers; Rust awaits each sink write.
Upstream auto-flushes pipes at a 16 KiB threshold and disposes slice instances;
this reusable Rust path instead final-flushes unconditionally on success and
leaves the destination caller-owned. It does not interpret `PipeWriter`
completion/cancellation flags, provide cancellation tokens, implement the
default C# encoder's broader Unicode allow-list, or add generated `@await`
syntax/HTTP response integration.

Delivery is tracked in [#37](https://github.com/davidfowl/razor-rust/issues/37)
under [#11](https://github.com/davidfowl/razor-rust/issues/11). Remaining
runtime cancellation/flush/encoder parity stays under #11; generated async
templates stay under [#3](https://github.com/davidfowl/razor-rust/issues/3)
and HTTP integration under [#6](https://github.com/davidfowl/razor-rust/issues/6).

The executable integration tests deliberately hold template work, writes and
flushes `Pending`, observe output before completion, inject write/mid-template/
final-flush/render failures, verify repeat/drop behavior, compare bounded
Unicode/escaping output with the existing encoder, and contrast the unchanged
buffered bridge. All-features tests exercise Tokio's partial-write path using
a one-byte duplex buffer.
