# Incremental async rendering

`render_async(&slice, &mut writer)` remains the **fully buffered** bridge for
synchronous `Slice`: render the complete `Vec<u8>`, then await one write and
one flush. It does not expose bytes during template execution.

`AsyncSlice` is a separate, reusable, statically dispatched async template
contract. `render_incremental` calls `render_into` with a borrowed
`AsyncHtmlWriter`, then awaits a final flush **only after success**.
Implementations write/await/flush in source order, propagating failures with
`?`; nested async slices use `output.write_slice(&child).await?` on the same
output, checking the completion threshold without an implicit final flush,
bypassing declarations but retaining explicit layout
composition. `render_into` deliberately selects full-document execution.
This does not change synchronous templates or layouts,
or require an executor in production default features. Generated typed
declarations now use this contract through
[`async_template!`](async-templates.md); synchronous `html!` stays unchanged.

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
section's prefix is observable before its later await completes. Typed
declarations and nested immediate-content section context are described in
[async-layouts.md](async-layouts.md). Generated templates can insert explicit
async sections using the grammar in [async-templates.md](async-templates.md).

## Automatic completion flush

Pinned upstream's `RazorSlice.cs:18,294-315` checks **unflushed output bytes
>= 16,384** after successful `ExecuteAsync`, including completed partials and
layout bodies. It does **not** flush when a write crosses the threshold or
before an arbitrary application await. Its TextWriter path does not auto-flush.
`RazorLayoutSlice.RenderSectionAsync` directly executes section code without
this lifecycle boundary; children rendered inside sections still have one.
`FlushTrackingPipeWriter.cs:54-77` counts advanced bytes and resets when a
flush is requested. The upstream lifecycle benchmark's `AutoFlushSlice` writes
20 KiB then completes; it is not a prefix-before-await threshold test.

The reversible Rust mapping uses the existing byte-oriented `AsyncSliceWriter`
as the pipe-like sink, without a new runtime, configurable policy or C# API
redesign. `AsyncHtmlWriter` counts bytes from successful writes through this
borrow, after UTF-8 encoding/escaping/formatting. Generated `@render`, handwritten
`write_slice`, supplied layout bodies and completed layout wrappers check the
inclusive threshold on the shared counter. Literal/value/attribute writes and
plain `write_section` dispatch do not. An explicit section association renders
its separate content slice as a child, so that child has a boundary.
Raw `render_into`/`render_body_into` are execution hooks; handwritten partial
calls should use `write_slice` to request the lifecycle boundary.

Explicit and automatic flushes reset accounting when requested, including
pending/failed flushes. Original errors stop execution; there is no final flush
after failure. Pending flush prevents later template code. Dropping a pending
render stops polling it without rollback or independently spawned cancellation.
The existing root `render_incremental` still final-flushes unconditionally on
success, including below threshold; the synchronous buffered bridge is unchanged.
No slice disposal or destination completion is added.

Accounting starts at zero on each new `AsyncHtmlWriter` borrow: bytes already
buffered by the caller, partial acceptance by a failing `write_all`, or writes
outside this output are not inspectable through the existing sink trait. A
saturating count preserves the threshold for arbitrarily large successful output.
The sink may expose bytes earlier than flush (Axum does); flush is not a network
receipt guarantee. Native PipeWriter counters/flags and TextWriter discrimination
are not claimed.

```sh
cargo run --locked -p razor-slices --example automatic_flush --features tokio
cargo test --locked -p razor-slices --test automatic_flush
cargo test --locked -p razor-slices-axum --test http real_http_completed_child
cargo run --locked -p razor-slices-axum --example automatic_flush_server
# Another terminal:
curl --no-buffer http://127.0.0.1:3000/stream
# After receiving the 16,384-byte x prefix, a third terminal:
curl -X POST http://127.0.0.1:3000/release
```

The executable's destination is a 64 KiB Tokio BufWriter over a 32-byte duplex,
so the entire 16,384-byte child prefix must be automatically flushed before
the reader releases later application work. There is no manual template flush,
sleep, eager render or producer task. It repeats the same page twice, then
renders it as associated section content; each run verifies all prefix bytes
and the exact encoded suffix. The [stdout fixture](../crates/razor-slices/tests/fixtures/automatic_flush.txt)
is checked by stable/MSRV CI. A real HTTP/1 test repeats the section path and
receives the exact prefix before release, then the exact complete ordered body;
that proves transport delivery, while the buffered-sink sample specifically
proves flush rather than mere sink acceptance.

Focused [#72](https://github.com/davidfowl/razor-rust/issues/72) is a native child
of #11 linked to #52, isolated on frozen #71. It does not integrate #68's request
token/context stack: that separately owned follow-up remains #69. These Rust
mapping choices are not claims of explicit maintainer API acceptance.

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
Upstream auto-flushes pipes at successful 16 KiB slice completion and disposes
slice instances; this reusable Rust path checks completed children/layouts and
also retains its stronger unconditional final-flush contract on root success,
and leaves the destination caller-owned. It does not interpret `PipeWriter`
completion/cancellation flags, provide cancellation tokens, implement the
default C# encoder's broader Unicode allow-list, or add HTTP response integration.
Generated declarations and explicit awaits are covered separately in
[async-templates.md](async-templates.md).

Delivery is tracked in [#37](https://github.com/davidfowl/razor-rust/issues/37)
under [#11](https://github.com/davidfowl/razor-rust/issues/11). Remaining
runtime cancellation/native-pipe/encoder parity stays under #11; generated async
templates are delivered by [#58](https://github.com/davidfowl/razor-rust/issues/58)
under [#3](https://github.com/davidfowl/razor-rust/issues/3)
and HTTP integration under [#6](https://github.com/davidfowl/razor-rust/issues/6).

The executable integration tests deliberately hold template work, writes and
flushes `Pending`, observe output before completion, inject write/mid-template/
final-flush/render failures, verify repeat/drop behavior, compare bounded
Unicode/escaping output with the existing encoder, and contrast the unchanged
buffered bridge. All-features tests exercise Tokio's partial-write path using
a one-byte duplex buffer.
