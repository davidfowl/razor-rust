# C# behavior parity

## Reference and acceptance

The reference is **DamianEdwards/RazorSlices at
`552937e5f2f21ebcdc73939be014f3d217ffbb22`**, dated 2026-09-28,
with `VersionPrefix` **0.12.1** in `src/Directory.Build.props`.
This is an exact source pin, not a claim that a NuGet release or moving `main`
was tested. It is the same reference used by the existing layout, async,
request-context docs and issue #66.

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
| Inclusive 16,384-byte completed-slice automatic flush | Implemented | `automatic_flush` suite and [source-grounded contract](async-rendering.md#automatic-completion-flush). Native prebuffered/partially accepted bytes and flush-result fidelity remain missing in #74. |
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
HTTP or pipe flushing.

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

Next bounded increment: source-pin executable cases for #74's native pending-byte
accounting and flush outcomes before designing optional sink capabilities.
Do not count #66, passing tests, or this matrix as completion of any parent epic.
