# C# behavior parity

## Reference and acceptance

The reference is **DamianEdwards/RazorSlices at
`552937e5f2f21ebcdc73939be014f3d217ffbb22`**, dated 2026-09-28,
with `VersionPrefix` **0.12.1** in `src/Directory.Build.props`.
This is an exact source pin, not a claim that a NuGet release or moving `main`
was tested. It is the same reference used by the existing layout, async,
request-context docs and issues #66/#74.

For each behavior, parity requires an observable contract: exact output,
dispatch/order, failure behavior, or a measured streaming boundary. Matching
API names and passing Rust-only tests do not establish C# equivalence.
The matrix distinguishes **verified** (executed against the pinned C# source),
**implemented** (Rust evidence, not differential acceptance), **missing**
(known remaining work), and **unsupported** (explicit current boundary).
An implemented row is not blanket verification of its whole feature.

## Evidence matrix

| Behavior and acceptance criterion | Status | Evidence / remaining scope |
| --- | --- | --- |
| Synchronous nested layouts: outer sees inner's own sections; provider can explicitly access its original child after body execution; no recursive fallback | Verified, bounded | `nested_layout_sections::pinned_csharp_golden_matches_immediate_context_repetition_and_partial_bypass` and the C# harness below execute the same three-layout document. Exact bytes and header/footer/banner counts match. |
| Optional, case-sensitive, repeated and whitespace section names; empty name rejected; partial bypasses all declared layouts | Verified, bounded | Same golden includes missing `page-only` (must not recurse), `Footer`, whitespace, repeated footer, bare partial and empty-name rejection. This covers ASCII literal output, not general encoder equivalence. |
| Layout references/boxes, `Sectioned` associations/shadowing, explicit composition, original writer errors, lazy execution without flush | Implemented | `tests/nested_layout_sections.rs` plus existing `declared_layouts`, `layouts`, `sections` suites. Every byte-offset failure of the golden document is tested, retaining I/O kind/source and exact accepted prefix. These Rust-specific adapter/error contracts are not C# differential tests. |
| Generated typed models, nested control flow, primitive/trusted/attribute values | Implemented | Runtime `inline`, `control_flow`, `async_templates`, `attributes` tests and runnable value examples; broader grammar/date/custom formatting and encoder configuration remain #3/#2. |
| Incremental async bodies, layouts and sections preserve ordering/backpressure | Implemented | `async_layouts`, `incremental_async`, `combined_async` tests and exact fixtures, plus async section unit tests; no new C# differential acceptance in this increment. See [async layout evidence](async-layouts.md). |
| Inclusive 16,384-byte completed-slice automatic flush | Implemented | `automatic_flush` suite and [source-grounded contract](async-rendering.md#automatic-completion-flush). No per-write/arbitrary-await flush; native byte-accounting cases below have differential evidence. |
| Native pending-byte snapshots override fallback counting, including prebuffered bytes, partially accepted failing writes and draining sinks | Verified, bounded | Eight C# cases in `NativeSinkCases.cs` exactly match `fixtures/native-byte-accounting.txt` and `native_byte_accounting::pinned_csharp_golden_matches_native_and_fallback_accounting`. Default fallback behavior remains unchanged. Native flush-result flags and TextWriter policy remain missing under #74. |
| Native flush-result flags versus faulted/cancelled operations; TextWriter auto-flush policy | Missing, reference characterized | The C#-only `flush-outcomes` mode below records twelve reference cases. These are acceptance inputs for a follow-up, not Rust differential verification or a new runtime capability. |
| Typed request scope and cancellation; buffered/streaming HTTP | Implemented | `request_context` and Axum transport tests. Broader request/layout assembly #69, framework service injection, portable response-start and lifecycle equivalence remain unverified/missing under #47/#52/#11. |
| Configurable .NET `HtmlEncoder`, broader Unicode/numeric/date formatting | Missing | Rust's five-character encoder and fixed primitive formatting are not byte-for-byte configurable C# policy parity. Requires dedicated differential cases and explicit compatibility decisions. |
| C# initialization, pooling and disposal lifecycle | Missing | Rust borrows/re-executes reusable templates; it does not emulate C# instance disposal/pooling. #47/#11/#52 remain open scope. |
| Razor files/`@section` syntax, async trait objects and local-only async futures | Unsupported | Current authoring is the accepted inline proc-macro mechanism, not Razor parsing. Upstream also avoids Razor `@section`; Rust sections are explicit providers. No additional authoring/API approval is implied. |

## Reproduce the C# golden

Requires Git and the .NET 10 SDK in addition to the Rust toolchain. The small
[harness](../tests/parity/Program.cs) compiles the pinned upstream runtime's
**unchanged** `src/RazorSlices/*.cs` directly, with the ASP.NET framework reference.
It does not copy upstream sources into this repository, substitute mocked
dispatch, restore a floating package or build the Razor source generator.
Handwritten C# slices/proxies exercise the real runtime's layout discovery,
`ContentSlice`, `RenderBodyAsync`, `RenderSectionAsync` and partial bypass.
This tests runtime behavior, not Razor code generation, package build metadata,
HTTP, actual network delivery or package/source-generator builds. The
`native-accounting` mode additionally exercises real upstream auto-flush
selection against an instrumented `PipeWriter`, not a real transport.

From the repository root, using an unused reference checkout path:

```sh
git clone https://github.com/DamianEdwards/RazorSlices.git ../RazorSlices-parity-reference
git -C ../RazorSlices-parity-reference checkout --detach 552937e5f2f21ebcdc73939be014f3d217ffbb22
mkdir -p target
dotnet run --project tests/parity/NestedLayoutSections.csproj \
  -p:RazorSlicesSource="$(cd ../RazorSlices-parity-reference && pwd)" \
  --verbosity quiet > target/csharp-nested-layout-sections.txt
diff -u crates/razor-slices/tests/fixtures/nested-layout-sections.txt \
  target/csharp-nested-layout-sections.txt
cargo test --locked -p razor-slices --test nested_layout_sections
```

The harness rejects a different HEAD or dirty runtime sources. The committed
[golden](../crates/razor-slices/tests/fixtures/nested-layout-sections.txt) is also
checked by ordinary Rust CI without requiring .NET. A successful explicit C#
run and diff are required before updating the golden or claiming new verified
parity. Existing Rust CI alone is not a fresh cross-language comparison.

## Synchronous section context (#66)

`Layout::render_layout_section(body, name, writer)` is the synchronous analogue
of `AsyncLayout::render_layout_section_into`. Its default validates the name
and emits nothing. A custom provider can insert `section(body, child_name)`
to explicitly forward to its own immediate content. Reference, box and
declared-layout adapters forward the hook. Render-local borrowed context
replaces the section-erasing closure; no full-body buffer or ambient mutable
state is introduced.

`with_section` now returns concrete `Sectioned<S, T>`, retaining `Slice` when
`S: Slice` and `Layout` when `S: Layout`, as the async association already does.
Outer associations win duplicate names; section content bypasses declared
layouts but preserves explicit compositions. Associations outside an already
declared page affect that page's consumers, not its original inner context.
This is a reversible Rust adapter choice, not a claim of accepted new syntax.

Next bounded increment: choose a bounded sink-policy mapping for the reference
cases below, preserving the distinction between result flags and failed operations.
Do not count #66, passing tests, or this matrix as completion of any parent epic.

## Native byte accounting (#74)

Pinned `FlushTrackingPipeWriter.Create` returns a destination unchanged when
`CanGetUnflushedBytes` is true. That native `UnflushedBytes` includes output
buffered before rendering; `RazorSlice.AutoFlush` checks it after successful
execution. Without the capability, the upstream wrapper starts at zero, counts
successful `Advance` calls, and resets when flush is requested.

`AsyncSliceWriter::unflushed_bytes() -> Option<usize>` supplies the analogous
optional snapshot. The default `None` preserves existing implementations;
`Some(count)` is the sink's authoritative current accepted/unflushed byte count,
not capacity or a lifetime total. In particular, `Some(0)` must not fall back
to successful-write accounting. A native count survives fresh output borrows
and partial acceptance before a write error/cancellation. The runtime only
queries it at existing successful child/layout completion boundaries.
No mandatory runtime/dependency, per-write flush, counter adapter for arbitrary
Tokio writers, implicit error recovery or new final-flush policy is added.
This is a reversible optional Rust capability, not claimed maintainer API acceptance.

Reproduce the eight C# cases after the reference checkout above:

```sh
dotnet run --project tests/parity/NestedLayoutSections.csproj \
  -p:RazorSlicesSource="$(cd ../RazorSlices-parity-reference && pwd)" \
  --verbosity quiet -- native-accounting > target/csharp-native-byte-accounting.txt
diff -u crates/razor-slices/tests/fixtures/native-byte-accounting.txt \
  target/csharp-native-byte-accounting.txt
cargo test --locked -p razor-slices --test native_byte_accounting --test automatic_flush
```

The [C# cases](../tests/parity/NativeSinkCases.cs) call the unchanged upstream
runtime with an instrumented native/non-native `PipeWriter`. They compare exact
flush counts at the threshold, empty-slice completion with prebuffered output,
a below-threshold snapshot, already-draining sinks, and native/fallback state
after a partially accepted failed write followed by a separate successful render.
The [golden](../crates/razor-slices/tests/fixtures/native-byte-accounting.txt)
also checks that failed execution itself does not auto-flush.
Rust-specific regressions additionally cover original errors, pending/failed/
dropped native flushes, cancellation after partial acceptance, generated children,
layouts, section-owned children, explicit/final flushes and reborrowed output.
A real Tokio `BufWriter` regression reports its actual buffered length, preloads
16,383 bytes and inserts a one-byte child over a one-byte duplex. The reader
must receive the entire threshold-sized prefix before releasing later template
work; without the native completion flush this handshake cannot complete.
Its buffer capacity exceeds the prefix, preventing an incidental capacity flush
from satisfying the test. This is bounded buffered-I/O evidence, not TCP delivery.
Boundary regressions also combine prebuffered bytes with generated escaped UTF-8,
numeric and conditional-attribute output at 16,383/16,384/16,385 bytes, checking
that the native count is queried once at completion rather than per value.

These cases do not equate .NET `PipeWriter.Advance` with every possible Rust
`write_all` implementation. A native provider must supply a truthful snapshot.
Default fallback cannot discover bytes the trait implementation does not report.
`flush -> io::Result<()>` still does not expose native `FlushResult` cancellation/
completion flags, and the byte sink still maps to pipe-like rather than
TextWriter-specific automatic-flush policy. Keep #74/#11/#52 open for that
remaining scope; the whole runtime is not at verified C# parity.

### Remaining flush outcomes and TextWriter policy

The same unchanged C# runtime is executable in `flush-outcomes` mode:

```sh
dotnet run --project tests/parity/NestedLayoutSections.csproj \
  -p:RazorSlicesSource="$(cd ../RazorSlices-parity-reference && pwd)" \
  --verbosity quiet -- flush-outcomes > target/csharp-flush-outcomes.txt
diff -u tests/parity/fixtures/flush-outcomes.txt target/csharp-flush-outcomes.txt
```

The [C#-only reference golden](../tests/parity/fixtures/flush-outcomes.txt)
characterizes twelve cases; it is **not** asserted by Rust CI or evidence that
Rust exposes native result flags:

- Both ready and held auto-flush operations complete rendering successfully
  for all four `FlushResult.IsCanceled`/`IsCompleted` combinations. The pinned
  renderer awaits completion but does not inspect these flags. A held flush
  still blocks render completion until released.
- A faulted flush preserves its original exception; an actually cancelled
  flush operation propagates cancellation. A nonfaulted result's canceled
  **flag** is therefore not equivalent to a cancelled operation.
- A `TextWriter` receiving 16,384 literal characters does not auto-flush;
  an explicit template flush invokes it once.

Do not invent a native-flag error policy from names alone or collapse actual
fault/cancellation into success. The current Rust sink has only
`flush -> io::Result<()>`, maps destinations to the existing pipe-like
completion policy, and cannot express a distinct TextWriter policy. Those
capabilities remain unimplemented; a follow-up must compare observable render
outcomes and explicit/final flushing before claiming parity.
