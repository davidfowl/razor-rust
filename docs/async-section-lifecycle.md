# Inherited async section lifecycle acceptance

This is a **tests-only** slice of [#47](https://github.com/davidfowl/razor-rust/issues/47),
not a new lifecycle API or completion of the layout epic. The unchanged runtime
already meets the bounded criterion. Existing body-only lifecycle probes and
inherited-section output tests did not combine section-local resource cleanup
with pinned initialization/disposal evidence.

The authority is
[RazorSlices 0.12.1 at `552937e5f2f21ebcdc73939be014f3d217ffbb22`](https://github.com/DamianEdwards/RazorSlices/tree/552937e5f2f21ebcdc73939be014f3d217ffbb22).
The independent C# project compiles that checkout's unmodified runtime source;
it rejects another commit or dirty runtime source. It targets the installed
.NET 10 SDK, not the reference checkout's development SDK or Razor generator.

## Criterion and executable topology

A page declares an inner layout, which declares an outer layout. The outer
renders the body, then inserts its immediate inner provider's `footer` twice.
Each footer dispatches `header` against the original page. Dispatch still works
after the inner/page bodies have returned. An unknown `unused` section does
nothing. Bare child execution runs only the page body, acquiring no section or
layout-body locals.

Successful output is exactly `OIPiFHhfFHhfo`. Each body/section invocation owns a
local test lease. Slots are **page body, inner body, outer body, footer, header**.
Success releases `1,1,1,2,2` locals. The trace records uppercase acquisition and
lowercase release, independently of output: `OIPpiFHhfFHhfo`. This checks nesting
and release order, not just total counts. Construction acquires no local lease.

The [fixture](../crates/razor-slices/tests/fixtures/async-section-lifecycle.txt)
has 48 shared `common/` rows, each measured independently with C# TextWriter and
PipeWriter and compared with Rust:

| Cases | Observable acceptance |
| --- | --- |
| Ready, first header held, second header held; success and all 13 write failures (42 rows) | Exact output prefix, original error identity, acquisition/release order, no suffix after error |
| Application error after releasing either held header (2) | Original application I/O error retained; active header/footer/outer locals all released |
| Cooperative cancellation in either held header (2) | Exact prefix, cleanup and no suffix; C# explicitly awaits with its propagated token |
| Two successful renders of the same page (1) | Sections execute again, not cached; fresh per-invocation locals |
| Bare child (1) | Only `P`; no section/local-layout acquisition |

Rust executes the 46 noncancelled shared cases through both legacy and scoped
entry points; the two cancellation cases use `render_incremental_with_context`.
Every attempt is followed by two healthy renders of the same caller-owned
declaration and destination. Owners remain live through success, errors,
cancellation and retry, and drop exactly once when the declaration is actually
dropped. The destination drops only when its caller drops it.

An additional Rust-only matrix holds either the first/second section await or
the write of its trailing `h`, then exercises legacy drop, scoped drop and
explicit scoped cancellation (12 cases). While held, only the outer/footer/header
locals remain live (`0,0,1,1,1`). Settlement drops those locals without dropping
owners, appending a suffix or flushing; the same declaration/destination can
then render successfully. A separate check drops an unpolled render without
acquiring any local resources. No scheduler timing or sleeps are used.

## Managed-only observations, not Rust lifecycle requirements

The fixture's 48 `csharp/` rows deliberately are **not** compared with Rust.
They count initializer callbacks, disposal override entries and idempotent
test-owned lease releases, in page/inner/outer order. The C# harness checks
initialization has occurred before the outer body executes. Source anchors:

- [`RazorSlice.cs`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.cs):
  `ExecuteAsyncImpl` and `RenderViaLayout` call `EnsureInitialized`; its
  `_initialized` guard invokes `Initialize` once per instance. Initialization is
  deferred until rendering, but all three declared providers initialize during
  descent, before outer body execution.
- [`RazorLayoutSliceOfTModel.cs`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorLayoutSliceOfTModel.cs):
  `RenderSectionAsync` calls immediate content's `ExecuteSectionAsync` directly,
  without another `ExecuteAsyncImpl`/initializer pass.
- `RazorSlice` successful rendering calls `Dispose`; `DisposeInternal` guards
  recursive content disposal. Failure is not an automatic owner-disposal
  guarantee. Explicit harness cleanup releases outstanding test-owned leases.

On success both footer/header callbacks execute twice **after their provider's
disposal override has run**. Initializers remain `1,1,1`; automatic disposal
override entries are `2,2,1`, while test-owned releases are `1,1,1`. On a write
failure at the first `O`, all three initializers already ran, but automatic
disposal/release counts are zero. On cancellation in a header, page and inner
test-owned leases were released by earlier successful body rendering; outer
cleanup still requires explicit owner action.

The same-instance rerender probe observes `init=1,2,2`, `init-order=PIOIO`:
the page's initialization is not repeated, whereas new layout instances are
created by the pinned proxies. This characterizes this probe, not a general
upstream guarantee that disposed templates/services are safely reusable.
Similarly the harness's idempotent test lease is not a guarantee that arbitrary
user `Dispose` overrides are idempotent.

Rust has no corresponding initializer/disposal hooks here. Render-local RAII
cleanup and actual owner drop are intentionally separate from these managed
observations; no synthetic public lifecycle API or automatic owner disposal
has been added.

## Reproduce

From the Rust repository root, with the reference checkout at `$RazorSlicesSource`:

```sh
dotnet run --project tests/async-section-lifecycle --configuration Debug \
  -p:RazorSlicesSource="$RazorSlicesSource" > /tmp/section-debug.txt
dotnet run --project tests/async-section-lifecycle --configuration Release \
  -p:RazorSlicesSource="$RazorSlicesSource" > /tmp/section-release.txt
diff -u crates/razor-slices/tests/fixtures/async-section-lifecycle.txt /tmp/section-debug.txt
diff -u /tmp/section-debug.txt /tmp/section-release.txt

cargo test -p razor-slices --test async_section_lifecycle \
  --test async_layouts --test layout_lifecycle
cargo test -p razor-slices --all-features --test async_section_lifecycle \
  --test async_layouts --test layout_lifecycle
cargo +1.75 test -p razor-slices --test async_section_lifecycle \
  --test async_layouts --test layout_lifecycle
cargo +1.75 test -p razor-slices --all-features --test async_section_lifecycle \
  --test async_layouts --test layout_lifecycle
```

The C# harness fails on a TextWriter/PipeWriter mismatch, wrong token/error
identity, incorrect held-live resources, failed cleanup or destination ownership.
Both Debug and Release output must match the committed 96-line fixture.

## Limits

No production code, dependencies, public API, generator or transport changes.
Pending async destination writes and dropping a Rust future have no synchronous
C# write/drop counterpart; their cases are explicitly Rust-only. Cancellation
of arbitrary C# application awaits requires cooperation, unlike racing a Rust
render scope. The shared error probe uses a recognizable application I/O error,
not a new generic exception contract. This does not establish generated Razor
initializer/service injection, framework-owned scopes, arbitrary disposal
overrides, native writer behavior, or full #47/#52/#11 parity.
