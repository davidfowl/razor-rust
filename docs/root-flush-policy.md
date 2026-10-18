# Opt-in raw-destination root completion

This is one bounded follow-up to [#74](https://github.com/davidfowl/razor-rust/issues/74)
and [#11](https://github.com/davidfowl/razor-rust/issues/11), not complete native
PipeWriter, TextWriter, HTTP or lifecycle parity.

## Contract

`AsyncSliceWriter::always_flush_on_root_completion()` defaults to `true`.
Existing writers, `TokioWriter`, HTTP transports and both incremental rendering
entry points retain their unconditional final flush after successful execution,
even for empty output or after a child/layout already flushed.

A raw-destination adapter can override that method to return `false`. Root
completion then uses the same automatic policy and shared accounting as
successful child/layout completion:

| Always flush root | Auto flush enabled | Successful root completion |
| --- | --- | --- |
| `true` (default) | Either | Await an unconditional final flush; no root counter query |
| `false` | `true` (default) | Await a flush only at **at least 16,384 unflushed bytes** |
| `false` | `false` | No automatic flush and no counter query |

The second row models raw C# PipeWriter completion; the third models raw C#
TextWriter completion. To select the third row, implement **both** policy hooks.
Returning `false` from `auto_flush_enabled()` alone still preserves the existing
unconditional root-final flush.

`unflushed_bytes()` remains authoritative when supplied, including prebuffered
and partially accepted failing writes. The fallback counts only successful
encoded-byte writes through the current output borrow, starting at zero and
resetting when a flush is requested. Writes, arbitrary awaits and plain section
dispatch never introduce a new flush boundary. Child/layout completion and
explicit template flushes are unchanged; the root does not append another
flush after them unless its selected policy requires one.

Opting out transfers responsibility for remaining buffered output to the caller.
A successful render may leave all output buffered. No flush, shutdown, completion
or disposal is performed when the borrowed destination is returned to its owner.
Call the destination's `flush()` explicitly when required. The buffered
`render_async` bridge still always writes and flushes, regardless of either hook.
Built-in transport adapters do not opt out.

Template/write/flush errors propagate without retries, cleanup flushes or success
fallbacks. Context cancellation continues to race the whole render and each I/O
operation, including a pending root-threshold flush. Dropping a render stops
polling; accepted output is not rolled back. No executor dependency is added.

## Exact reference

The reference is unchanged **DamianEdwards/RazorSlices
`552937e5f2f21ebcdc73939be014f3d217ffbb22`**, version prefix **0.12.1**.
In [`RazorSlice.cs`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.cs),
`RenderToPipeWriterAsync` calls `AutoFlush` only after successful execution;
`AutoFlush` checks inclusive `UnflushedBytes >= 16_384`.
`RenderToTextWriterAsync` does not flush after synchronous success, and its
asynchronous continuation's `AutoFlush` has no pipe to flush.
Neither raw destination overload adds an unconditional root-final flush.
[`RazorSlice.Partials.cs`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.Partials.cs)
shares that destination with children.
[`FlushTrackingPipeWriter.cs`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/FlushTrackingPipeWriter.cs)
uses native counting when available and otherwise starts a wrapper counter at
zero. HTTP result/stream overloads are separate contracts, not the reference for
this opt-in.

## Executable evidence

`tests/root-flush-policy/RootFlushPolicy.csproj` compiles the pinned runtime
sources directly against the .NET 10 shared framework, like the existing parity
harness. It rejects a different reference HEAD or dirty runtime source and does
not modify the upstream checkout or use a substitute renderer.

The 66-row shared fixture covers immediate and explicitly held execution for
native PipeWriter, fallback PipeWriter and TextWriter destinations:

- Empty and 16,383/16,384/16,385-byte root bodies.
- Encoded values that reach exactly 16,384 output bytes.
- Two completed children followed by an unflushed root suffix, versus a root
  that crosses the threshold after a below-threshold child.
- Explicit flush followed by an unflushed suffix, and declared layout completion
  without a duplicate root flush.
- Prebuffered empty/one-byte roots, distinguishing native from fallback counters.

Each implementation asserts the entire expected output independently; the
fixture compares encoded-byte length, exact flush sizes/order and remaining
pending bytes. The Rust test repeats all rows through both incremental entry
points. The C# harness also asserts original template failures do not flush,
for immediate/held execution and both destinations; pending root flush faults
and actual cancellation must propagate rather than render successfully.

Additional dedicated Rust regressions cover the default unconditional policy,
original write/template/explicit/child/root errors, partial failure accounting
on a subsequent render, cancellation before execution, held application work,
partially accepted pending writes, explicit/child/root flush backpressure, drop,
and caller-owned destination reuse. The all-feature test uses a real oversized
`BufWriter` over a one-byte duplex: a threshold root awaits delivery, whereas a
below-threshold root completes with output still buffered until the caller flushes.

Before implementation, `raw_text_root_does_not_add_a_final_flush` failed against
the unchanged runtime because successful raw text rendering added one unwanted
flush. With the opt-in it passes; default-policy regressions still require that
flush.

From the Rust repository root, with a clean reference checkout:

```sh
git clone https://github.com/DamianEdwards/RazorSlices.git ../RazorSlices-root-reference
git -C ../RazorSlices-root-reference checkout 552937e5f2f21ebcdc73939be014f3d217ffbb22
dotnet run --project tests/root-flush-policy/RootFlushPolicy.csproj \
  -p:RazorSlicesSource="$(cd ../RazorSlices-root-reference && pwd)" \
  --verbosity quiet > target/csharp-root-flush-debug.txt
dotnet run --project tests/root-flush-policy/RootFlushPolicy.csproj -c Release \
  -p:RazorSlicesSource="$(cd ../RazorSlices-root-reference && pwd)" \
  --verbosity quiet > target/csharp-root-flush-release.txt
diff -u crates/razor-slices/tests/fixtures/root-flush-policy.txt target/csharp-root-flush-debug.txt
diff -u crates/razor-slices/tests/fixtures/root-flush-policy.txt target/csharp-root-flush-release.txt
cargo test --locked -p razor-slices --test root_flush_policy
cargo test --locked -p razor-slices --test root_flush_policy --all-features
```

Create `target/` first if no Cargo command has run yet. Run the harness from the
Rust checkout, not under the reference's unrelated SDK-pinning `global.json`.

## Remaining gaps

This policy does **not** represent native `FlushResult` flags, implement C#
initialization/pooling/disposal or managed cancellation/lifecycle equivalence,
change TextWriter character encoding, configure the broader C# HTML encoder,
or establish HTTP response-start/completion equivalence. Native/fallback counter
limitations remain as documented in [the existing parity evidence](parity.md).
Rust cancellation/drop and error assertions are Rust contracts, not differential
proof of all C# cancellation or disposal paths. #74 and #11 remain open.
