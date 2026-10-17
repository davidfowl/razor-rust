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
| Generated async writer type does not shadow identifiers visible in the invocation | Implemented, Rust-specific | `compiler_hygiene` renders associated constants twice with exact escaped output; macro unit coverage includes nested/raw identifiers and generic names. Identifiers introduced by later nested macro expansion are outside this scan. No C# differential claim. |
| Raw model field spelling and conditional-compilation validation | Implemented, Rust-specific | `compiler_raw_identifiers` checks owned/borrowed constructors and render-body compilation; macro unit tests accept `r#model`, reject wrong/multiple fields and retain the invocation-level `cfg`/`cfg_attr` guard for ordinary/raw spellings on structs/fields. This is compiler compatibility, not runtime/lifecycle equivalence. |
| Render-local code blocks preserve bindings and semicolon-less unit expression tails | Implemented, Rust-specific | `compiler_code_blocks` renders exact escaped output twice, with zero construction effects and five effects per render, including nested/attributed/awaited tails. Four standalone doctests include positive binding scope and rejection of integer, `Result` and unawaited future tails. No C# differential or lifecycle equivalence claim. |
| Text bridge discards completed/invalid pending UTF-8 after errors without panicking or replaying bytes on explicit cleanup writes | Implemented, Rust-specific | `render_lifecycle` covers every split of selected two/three/four-byte scalars, original formatting/UTF-8 error types, malformed pending sequences and repeated rendering. This repairs the inline #30 bridge's error state, not the C# encoder policy. |
| Render-local resources drop after write/flush errors while model/layout captures and destinations remain owned by callers | Implemented, bounded | Four `render_lifecycle` tests include synchronous text/byte errors and nested generated async layout reuse after write/final-flush errors. Original I/O kind/source identity, accepted prefix, local/captured/destination drop counts and persistent application effects are checked. No cancellation/token acceptance is included. |
| Optional body values: absent output, encoded string, nullable integer/float and trusted HTML | Verified, bounded | Twelve `optional-values` cases use both pinned C# PipeWriter/TextWriter paths and match `fixtures/optional-values.txt` in synchronous text/byte and generated async Rust paths. Rust `Option<T>` delegates `Some` and emits nothing for `None`. This does not establish broad encoder/culture/boolean formatting equivalence. |
| Optional value reuse, nesting/references, custom contracts, original errors, held writes, cancellation/drop and bounded encoding | Implemented | Seven `optional_values` regressions plus two compile-fail doctests reject unsupported contained types. No arbitrary Display/synchronous fallback, implicit flush, or attribute omission change. Every document-byte failure retains its original I/O source and exact accepted prefix. |
| Character body/nullable/attribute output in the selected common ASCII subset | Verified, bounded | Fourteen `character-values` cases match actual pinned C# PipeWriter/TextWriter output and generated sync text/byte/async Rust paths via `fixtures/character-values.txt`. Character support is additive; broader Unicode/quote-policy equivalence is not claimed. |
| Character UTF-8/string-encoding consistency, original failures, backpressure, cancellation/drop and byte thresholds | Implemented | Eight `character_values` regressions cover Unicode scalar boundaries, optional/reference attributes, every failure offset including partial UTF-8, final-flush errors, held writes and 16,383/16,384/16,385-byte native/fallback thresholds. Existing bool/float representations stay unchanged. |
| Boolean/float/quote/Unicode reference behavior beyond the shared subset | Missing, reference characterized | Eighteen C#-only `primitive-outcomes` cases below expose direct/generic boolean path differences and default formatting/encoder policy. These are not Rust differential verification or permission to copy a reference defect/change Rust defaults. |
| Incremental async bodies, layouts and sections preserve ordering/backpressure | Implemented | `async_layouts`, `incremental_async`, `combined_async` tests and exact fixtures, plus async section unit tests; no new C# differential acceptance in this increment. See [async layout evidence](async-layouts.md). |
| Inclusive 16,384-byte completed-slice automatic flush | Implemented | `automatic_flush` suite and [source-grounded contract](async-rendering.md#automatic-completion-flush). No per-write/arbitrary-await flush; native byte-accounting cases below have differential evidence. |
| Native pending-byte snapshots override fallback counting, including prebuffered bytes, partially accepted failing writes and draining sinks | Verified, bounded | Eight C# cases in `NativeSinkCases.cs` exactly match `fixtures/native-byte-accounting.txt` and `native_byte_accounting::pinned_csharp_golden_matches_native_and_fallback_accounting`. Default fallback behavior remains unchanged. Native flush-result flags remain unrepresented under #74. |
| Pipe versus TextWriter-like completion policy at 16,383/16,384/16,385 bytes, nested partials, explicit flush and layout completion | Verified, bounded | Twelve C# `flush-policy` cases match shared `fixtures/flush-policy.txt` and `flush_policy::pinned_csharp_golden_matches_completion_policy_not_root_final_flush`, with both Rust native/fallback counting. Default enabled; TextWriter-like opt-out suppresses completion checks, not explicit/root-final/bridge flushes. Root-final-flush contracts still differ. |
| Opt-out through generated children, nested declared/explicit layouts and layout-owned section children; original failures, held flushes, cancellation/drop | Implemented | Six `flush_policy` regressions include these Rust adapter/error contracts, unchanged explicit/root-final/bridge flushing, and no native-counter query when disabled. These cases do not establish C# error/lifecycle equivalence. |
| Native flush-result flags versus faulted/cancelled operations | Missing, reference characterized | The C#-only `flush-outcomes` mode below records twelve reference cases. Native flags are not exposed by Rust; the TextWriter completion subset now has separate bounded differential evidence above. |
| Typed request scope and cancellation; buffered/streaming HTTP | Implemented | `request_context` and Axum transport tests. Broader request/layout assembly #69, framework service injection, portable response-start and lifecycle equivalence remain unverified/missing under #47/#52/#11. |
| Configurable .NET `HtmlEncoder`, broader Unicode/numeric/date formatting | Missing | Rust's five-character encoder and fixed primitive formatting are not byte-for-byte configurable C# policy parity. Requires dedicated differential cases and explicit compatibility decisions. |
| C# initialization, pooling and disposal lifecycle | Missing, reference characterized | Eight C#-only `render-lifecycle` cases measure initialization, local cleanup, `Dispose` override calls, destination ownership and explicit same-instance retry after success/error. Rust retains ordinary ownership rather than copying per-render disposal. Pooling/layout disposal cascades and framework/service lifetime equivalence remain unverified under #47/#11/#52. |
| Razor files/`@section` syntax, async trait objects and local-only async futures | Unsupported | Current authoring is the accepted inline proc-macro mechanism, not Razor parsing. Upstream also avoids Razor `@section`; Rust sections are explicit providers. No additional authoring/API approval is implied. |

## Rust compiler compatibility

Compiler-only fixes are integrated alongside the character work, without
changing the pinned C# reference or Rust runtime defaults. Their source patches
are `c8107ded48af02f879a726a5c88b52b6cab6fa23` (writer-name hygiene) and
`56b7d0609f9c3c7a7a0f1e2d5f60bf1dd0177b93` (raw-identifier validation), plus
`7370e5ed641f22d207e3beab757eb6e8bfa7eca7` (unit code-block tails).
They were reviewed against their common base
`841bf9c585209cc3415d5fdd0e1224f133ccb852` and integrated over the character
implementation. These are Rust-specific bugs, not cross-language comparisons.

Before the fixes, the new tests fail against character head `5eb0c970`:
`{__RazorWriter::TEXT}` produces `E0599` because the generated writer type
parameter shadows the user's type, and `r#model` is rejected as though it were
not the required `model` field. The corrected code scans visible invocation
identifiers, including nested/raw names, and normalizes raw spellings during
declaration validation. It retains the existing prohibition on inner
conditional-compilation attributes rather than allowing raw spellings to
bypass that guard.

```sh
cargo test --locked -p razor-slices-macros
cargo test --locked -p razor-slices --test compiler_hygiene --test compiler_raw_identifiers --test compiler_code_blocks
```

The hygiene regression renders the same template twice and requires exact
`&lt;first&gt;&amp;second&gt;third` output. The raw-field regression compiles both
body spellings and owned/borrowed typed constructors; it does not execute a
render. Three additional macro unit tests cover name selection, valid/invalid
model declarations, and ordinary/raw struct/field conditional attributes.
The name scan cannot observe identifiers introduced only by a later nested
macro expansion. Broader grammar and parent-epic scope remain open.

The code-block regression also fails against the earlier combined compiler head
`7d0149ce`: a valid `@{ let label = "<local>"; model.touch() }` produces an
unexpected generated `__razor_writer` token when its tail is flattened directly
before the next template write. Wrapping only semicolon-less expressions as
unit-valued block statements preserves preceding render-local bindings without
silently discarding values. The regression renders `&lt;local&gt;done` twice,
asserting zero effects at construction and effect counts of five and ten after
the two renders. Its four doctests preserve rejection of integer, `Result`
and unawaited future tails. This is Rust code-generation correctness, not new
cross-language or cancellation/lifecycle parity.

## Render lifecycle and error-state cleanup

The live #11 discussion leaves initialization/disposal and writer lifecycle
parity open. #47 covers section/layout context and error boundaries; #52
separately covers HTTP peer disconnect, request cancellation and response start.
None requires copying a managed pooling/disposal architecture into Rust.
The existing inline #30 contract also includes split UTF-8 and writer errors.
This increment addresses a demonstrated bridge error-state bug and records
bounded ownership evidence; it does not complete any of those parent scopes.

Before the bridge fix, an error forwarding a completed four-byte code point
left `TextWriter.len == 4`. An explicitly handled error followed by a cleanup
write indexed outside its four-byte array. Malformed pending UTF-8 could leave
the same invalid state. Completed two/three-byte code points could instead
leak into a later cleanup write. The bridge now clears completed/invalid state
before returning the error, without changing error types, accepted output,
validation of malformed input, or normal incomplete-code-point buffering.
Generated rendering still stops at the first error. The regressions explicitly
attempt cleanup inside handwritten code and then return the original error;
there is no implicit retry, output rollback or error swallowing.

Four Rust regressions in `tests/render_lifecycle.rs` cover every split of
selected two/three/four-byte scalars, malformed pending sequences, repeated
renders, and ordinary resource ownership. Synchronous closures and generated
nested async layouts release render-local guards on write/final-flush errors,
but retain owned model/layout captures until their owner is dropped. Borrowed
destinations remain open. The same template can be rendered again; application
effects persist and are not reset or rolled back. These are Rust contracts,
not a new disposal callback API or cancellation acceptance.

The new **C#-only** `render-lifecycle` mode executes eight cases against the
unchanged pinned runtime: PipeWriter/TextWriter, synchronous/explicitly held
execution, and success/write failure, each followed by a new-destination render
of the same instance. It checks exception identity and records:

- One initializer call, two executions and two render-local `using` disposals.
- Two automatic `Dispose` override calls for two successful renders, but one
  for a failed render followed by success. This counts override invocations,
  not physical resource releases; a fault is not an automatic slice-disposal
  guarantee in these paths.
- No writer disposal/completion by rendering; each caller-owned writer ends
  only when the harness explicitly disposes/completes it.

Re-rendering the C# instance after a previous automatic `Dispose` is measured
characterization, not a general upstream reuse guarantee. Rust deliberately
retains reusable borrowed/owned templates and normal `Drop`, rather than
emulating the reference's per-render calls or fault behavior. Pool reuse,
layout disposal cascades, service scopes, HTTP ownership and cancellation
remain separate acceptance. Any future managed-style lifecycle hook needs a
specific use case and compatibility decision, not inference from this fixture.

Using the pinned checkout described below:

```sh
dotnet run --project tests/parity/NestedLayoutSections.csproj \
  -p:RazorSlicesSource="$(cd ../RazorSlices-parity-reference && pwd)" \
  --verbosity quiet -- render-lifecycle > target/csharp-render-lifecycle.txt
diff -u tests/parity/fixtures/render-lifecycle.txt target/csharp-render-lifecycle.txt
cargo test --locked -p razor-slices --test render_lifecycle --test inline
```

The eight reference rows match both Debug and Release. They are not a shared
Rust/C# lifecycle golden or new full initialization/pooling/disposal parity.
No #69 request/layout cancellation tests, token contracts or fixtures change.

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

Remaining native-result work must resolve a bounded capability and
verify ready/held outcomes against the reference cases below, preserving the
distinction between result flags and failed operations.
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
queries it at existing successful child/layout completion boundaries when automatic
flushing is enabled.
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
completion flags. Completion opt-out below does not make Rust root-final-flush
or slice/destination lifecycle identical to C#. Keep #74/#11/#52 open for
remaining scope; the whole runtime is not at verified C# parity.

### TextWriter-like completion policy

The pinned C# TextWriter path does not auto-flush at slice completion.
`AsyncSliceWriter::auto_flush_enabled() -> bool` defaults to `true`, retaining
all existing pipe-like sinks. An adapter can return `false` to suppress the
shared child/layout completion check, including native-counter queries.
No sink-type guessing or configurable threshold is added.

Reproduce the twelve shared policy cases:

```sh
dotnet run --project tests/parity/NestedLayoutSections.csproj \
  -p:RazorSlicesSource="$(cd ../RazorSlices-parity-reference && pwd)" \
  --verbosity quiet -- flush-policy > target/csharp-flush-policy.txt
diff -u crates/razor-slices/tests/fixtures/flush-policy.txt target/csharp-flush-policy.txt
cargo test --locked -p razor-slices --test flush_policy
```

The C# harness uses the real PipeWriter and TextWriter render paths; Rust
uses completion boundaries on `AsyncHtmlWriter`, **not** `render_incremental`.
Both verify exact ASCII output and compare flush counts below/at/above the
threshold, two half-sized partials, explicit flush and a half-body/half-layout
document. Rust repeats the golden for native and fallback accounting.
Six focused Rust regressions additionally verify generated children, nested
declared/explicit layouts, repeated layout-owned section children, no native
queries while disabled, accepted output/error sources, held explicit/final
flushes, cancellation and dropping the render.

Explicit flushes, the root `render_incremental` final flush and the buffered
`render_async` bridge remain mandatory on success regardless of this hook;
their errors and scoped cancellation remain failures, not successful fallbacks.
Raw C# TextWriter root rendering has no corresponding final flush. This is
bounded **completion-policy** parity, not identical root or lifetime semantics.
The additive hook is a reversible adapter mapping, not claimed maintainer
API acceptance.

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
`flush -> io::Result<()>` and cannot represent native result flags. The
TextWriter-like completion opt-out above does not expose those flags or change
the intentionally preserved root-final-flush contract. A follow-up must compare
observable render outcomes before claiming further parity.

## Optional body values (#78)

The pinned runtime's `RazorSlice.Write.cs` skips null strings/content and
null generic values; non-null values retain their existing typed dispatch.
Before this increment, both `html!` and `async_template!` rejected
`Option<&str>` body interpolation with `E0277`, even though conditional
attributes already had their own `Option` contract.

`WriteHtmlValue for Option<T>` and `AsyncWriteHtmlValue for Option<T>` are
additive delegating implementations, not new syntax or a formatting API.
`None` performs no destination I/O; `Some` borrows the value and preserves its
encoding, trust, fixed scratch bounds, original errors and awaited backpressure.
The contained type must implement the relevant contract, even for `None`;
there is no `Display` coercion or sync-to-async buffer. Rust-specific nesting,
references and custom explicit implementations compose without cloning.
Body `Some(false)` still emits Rust's existing `false`, not an omitted attribute.
Cancellation still interrupts an enclosing render of absent values, and root
final flushes remain unchanged. Attribute omission rules are untouched.

Reproduce the twelve shared cases against the same pinned source:

```sh
dotnet run --project tests/parity/NestedLayoutSections.csproj \
  -p:RazorSlicesSource="$(cd ../RazorSlices-parity-reference && pwd)" \
  --verbosity quiet -- optional-values > target/csharp-optional-values.txt
diff -u crates/razor-slices/tests/fixtures/optional-values.txt target/csharp-optional-values.txt
cargo test --locked -p razor-slices --test optional_values
```

The [C# harness](../tests/parity/OptionalValueCases.cs) executes both actual
PipeWriter and TextWriter render paths and requires identical output before
printing the shared golden. Cases cover null/empty/encoded ASCII strings,
null/zero/negative/minimum/maximum integers, absent/trusted HTML including
raw UTF-8, and an absent/present `1.5` double under invariant culture.
Rust compares exact output on generated sync text/byte and async paths.
The [golden](../crates/razor-slices/tests/fixtures/optional-values.txt) is checked
by Rust CI; updating it requires an explicit unchanged-source C# run.

Seven Rust regressions additionally cover reusable owned and non-Clone custom
values, references/nesting, absent values against a failing destination,
every accepted-byte failure offset, original final-flush errors, held writes,
cancellation/drop and bounded encoding/completion accounting. Two compile-fail
doctests retain rejection of unsupported optional payload types.
This is bounded nullable-value dispatch/output evidence, not full .NET encoder,
culture/date/number/boolean formatting or instance-lifecycle equivalence.

## Character values and primitive outcomes (#79)

Before selecting the runtime change, the unchanged pinned C# runtime was
executed on both PipeWriter and TextWriter destinations. It supports character
body/nullable/attribute output, while both Rust generated body paths and
`attribute(name, char)` failed with `E0277`. The implementation adds `char` to
`WriteHtmlValue`, `AsyncWriteHtmlValue` and the existing attribute value adapters.
A four-byte UTF-8 stack buffer feeds the existing string encoder; it does not
allocate, add syntax, bypass encoding or flush implicitly. References and
optional values retain their existing delegation/omission contracts.

Reproduce the fourteen shared character cases:

```sh
dotnet run --project tests/parity/NestedLayoutSections.csproj \
  -p:RazorSlicesSource="$(cd ../RazorSlices-parity-reference && pwd)" \
  --verbosity quiet -- character-values > target/csharp-character-values.txt
diff -u crates/razor-slices/tests/fixtures/character-values.txt target/csharp-character-values.txt
cargo test --locked -p razor-slices --test character_values
```

The [value harness](../tests/parity/OptionalValueCases.cs) requires actual C#
pipe/text output to agree for the shared cases: selected ASCII characters,
ampersand/angle-bracket/double-quote escapes, nullable characters and scalar/
optional character attributes. Rust compares generated sync text/byte and
async output with the same [golden](../crates/razor-slices/tests/fixtures/character-values.txt).
The subset is not all ASCII or full .NET encoder equivalence.

Rust `char` is a Unicode scalar, not a UTF-16 code unit; isolated surrogates
cannot be represented. UTF-8 boundaries, controls, supplementary characters and
the maximum scalar are tested against the **existing Rust string encoder**,
not asserted equivalent to .NET's broader default encoder. The old negative
`char` compiler case is now `Vec<char>`: adding scalar support does not add
implicit collection or `Display` conversion.

Eight regressions additionally preserve original I/O kind/source and accepted
prefix at every body/attribute failure byte, including partial four-byte UTF-8,
skip suffix/final flush after failure, retain final-flush errors and held-write
backpressure/cancellation/drop, and count encoded versus UTF-8 bytes exactly at
16,383/16,384/16,385-byte completion boundaries with native/fallback accounting.

### Reference-only primitive characterization

The same source was characterized before choosing the additive character fix:

```sh
dotnet run --project tests/parity/NestedLayoutSections.csproj \
  -p:RazorSlicesSource="$(cd ../RazorSlices-parity-reference && pwd)" \
  --verbosity quiet -- primitive-outcomes > target/csharp-primitive-outcomes.txt
diff -u tests/parity/fixtures/primitive-outcomes.txt target/csharp-primitive-outcomes.txt
```

These [eighteen C#-only cases](../tests/parity/fixtures/primitive-outcomes.txt)
are **not** asserted by Rust CI as byte parity:

- Direct `Write(bool?)` produces no boolean bytes on PipeWriter, but title-case
  `True`/`False` on TextWriter. Generic `Write<T>(bool)` produces title-case
  output on both. The pinned `PipeWriterHtmlExtensions.Write(bool)` formats
  into a span without advancing it. Rust deliberately retains its existing
  lowercase `true`/`false`; it does not emulate lost reference output.
- Under invariant culture, C# infinities are `Infinity`/`-Infinity`, while Rust
  retains `inf`/`-inf`. Both measured negative-zero cases render `-0`.
  C# uses `1E-30` and encoded `1E&#x2B;30` for the selected magnitudes; Rust
  retains its existing decimal `Display` representation.
- C# single quote is `&#x27;`, where Rust strings/characters retain `&#39;`.
  Both measured double-quote paths produce `&quot;`. C# encodes the selected
  BMP/supplementary scalars as `&#x96EA;`/`&#x1F980;`; Rust retains its existing
  UTF-8 text policy.

Broader primitive/encoder compatibility therefore requires an explicit bounded
policy decision and its own differential cases, not a blanket default-format
change or copying the direct-boolean path discrepancy. This increment leaves
all existing Rust representations and public APIs intact.
