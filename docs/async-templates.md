# Typed declared async templates

`async_template!` generates a concrete model-typed `AsyncSlice`, not a buffered
`html!` result or a captured one-shot future. This is the bounded generated
declaration/async milestone [#58](https://github.com/davidfowl/razor-rust/issues/58)
under [#3](https://github.com/davidfowl/razor-rust/issues/3), using the incremental
runtime tracked in [#11](https://github.com/davidfowl/razor-rust/issues/11).
[#62 / PR #63](https://github.com/davidfowl/razor-rust/pull/63) adds typed
bounded value parity on frozen integration #61 head
`7b6a8aeac067a06f64eae656a79ccdea5b035619`, carrying its unmerged dependencies.
Its PR is cumulative over the unmerged control-flow and async-section stacks;
this document describes the branch, not features already landed on main.

## Declaration and grammar

```rust
use razor_slices::async_template;

async_template! {
    pub struct Greeting<'a> { model: &'a str } => {
        "<p>Hello, " {model} "!</p>"
        @flush;
        @await { async {} };
    }
}

let greeting = Greeting::new("<World>");
// render_incremental(&greeting, &mut writer).await?;
```

The declaration is a Rust named-field struct with exactly one field named
`model`. The generated `new(model)` constructor has the struct's visibility.
The field's type is the constructor's type: it can own a model or borrow one
with an explicit lifetime. Each render binds `model` to `&self.model`; it does
not consume it. Ordinary lifetime/type parameters, where clauses, derives and
documentation attributes are supported. Supply `Sync` bounds for generic
models, e.g. `struct Text<T: AsRef<str> + Sync> { model: T }`. Conditional
compilation must wrap the whole macro invocation; `cfg`/`cfg_attr` on the
inner struct/model field are rejected rather than leaving orphan generated
implementations or conditionally removing the required model field.

Every body (including nested branches/loops) accepts:

| Syntax | Meaning |
| --- | --- |
| `"trusted markup"` or a raw string literal | Await an unescaped literal write |
| `{ RustExpression }` | Evaluate and await typed `AsyncWriteHtmlValue` output |
| `if condition { body } else if condition { body } else { body }` | Ordinary Rust conditional selection, including `if let` |
| `for pattern in expression { body }` | Ordinary Rust iteration with scoped bindings |
| `@{ Rust statements }` | Render-local bindings, application work and Rust awaits; bindings remain in the containing template scope |
| `@await { future };` | Await a unit-returning application future |
| `@await { future }?;` | Await a unit-result future and propagate its convertible error |
| `@flush;` | Await a flush at this exact insertion point |
| `@render { child };` | Execute an `AsyncSlice` on the same writer, without an implicit flush |
| `@section { provider }, { name };` | Await explicit lazy section lookup/rendering on the same writer |

Await directives deliberately require `()` (or `Result<(), E>` with `?`):
neither unused async values nor ignored `Result`s are silently discarded.
For values, use `@{ let value = fetch(model).await?; }` followed by `{value}`,
or `{fetch(model).await?}`. Normal Rust diagnostics still apply to code-block
statements; the macro does not suppress must-use warnings.

Interpolation uses the explicit `AsyncWriteHtmlValue` contract. Built-ins are
`str`, `String`, references, `bool`, all Rust signed/unsigned integer families,
`f32`/`f64`, trusted `HtmlString`, and validated `attribute(name, value)` adapters.
Strings escape `& < > " '` once and preserve UTF-8, using the existing encoder
in at most 1,024-byte input chunks and a fixed 6,144-byte scratch buffer.
Integers share the synchronous 40-byte format buffer; floats share the
400-byte format buffer without the synchronous heap fallback (format overflow
returns an `InvalidData` I/O error). Built-in formatting is the existing Rust
representation, including lowercase body `true`/`false`, negative zero,
infinities and NaN; it is not a new C# culture/format policy.

Trusted `HtmlString` is borrowed and emitted verbatim, like literal markup,
without output copying. Literal/trusted/name writes can borrow arbitrarily long
inputs directly; the 6,144-byte encoded-write bound is not a maximum size for
these raw writes. Scratch is independent of input length, not a bound on
application-owned model memory, sink storage or custom implementation behavior.
The original string-only `AsyncHtmlWriter::write_value` API remains intact.
`char` is not currently a synchronous `WriteHtmlValue` type and is not added.

`attribute` keeps its existing validation and omission rules: true repeats the
name, false/`None` omit the entire attribute, `Some` and references delegate,
and empty strings retain ` name=""`. Text/numeric/trusted values use the typed
async path. Invalid names fail construction even for omitted values; there is
no URL normalization or sanitization. `HtmlString` deliberately bypasses
encoding in attributes too: callers must ensure it is already safe inside
double quotes, not merely safe as body markup. See the
[attribute contract](../crates/razor-slices/ATTRIBUTES.md).

Unsupported types fail type checking; neither `Display` nor synchronous
`WriteHtmlValue` is an async fallback. Custom values opt into
`AsyncWriteHtmlValue`, and custom conditional values additionally implement
`AsyncAttributeValue` alongside the existing `AttributeValue` omission contract.
Implementations own encoding/trust, bounded memory and awaited `Send` I/O;
there is no arbitrary sync-value buffer, nested executor or hidden flush.

Headers follow the existing [control-flow grammar](../crates/razor-slices/CONTROL_FLOW.md);
parenthesize struct literals in conditions/iterators. Unsupported markup,
missing directive delimiters and invalid declarations fail at compile time,
never becoming silent text. The synchronous `html!` grammar/behavior is unchanged.
The macro supplies its runtime path hygienically, including renamed dependencies
and callers without the implicit prelude.

## Runnable prefix-before-await fixture

```shell
cargo run -p razor-slices --example async_template --features tokio > target/async-template-output.txt
diff -u crates/razor-slices/tests/fixtures/async_template.txt target/async-template-output.txt
```

Exact stdout:

```text
prefix-before-await: <h1>&lt;Todos&gt;</h1>
prefix-before-await: <h1>&lt;Todos&gt;</h1>
page: <h1>&lt;Todos&gt;</h1>
<p>&lt;loaded&gt;</p><ul><li>Read &amp; learn</li><li><s>Ship &lt;Rust&gt;</s><small>&quot;done&quot;</small></li></ul>
renders: 2
```

The [declaration](../crates/razor-slices/examples/support/async_template.rs)
owns a typed model containing borrowed rows and a `Notify`. It writes and flushes
the heading, awaits application work producing a string, then executes nested
row conditionals/loops with escaped model fields.
The [executable](../crates/razor-slices/examples/async_template.rs) uses an
8-byte duplex buffer. Its reader asserts the entire heading and unchanged
application completion count **before** notifying the later work to proceed.
Both renders use the same template/model and create fresh awaited work.
There is no sleep, hidden executor, buffering of a synchronous template, or
spawn in generated code. Buffering the whole page would deadlock the handshake.

## Mixed-value prefix-before-await fixture

```shell
cargo run -p razor-slices --example async_values --features tokio > target/async-values-output.txt
diff -u crates/razor-slices/tests/fixtures/async_values.txt target/async-values-output.txt
```

Exact stdout:

```text
prefix-before-await: <h1 data-count="2" title="&lt;Values&gt;">&lt;Values&gt;true<b>trusted</b></h1>
prefix-before-await: <h1 data-count="2" title="&lt;Values&gt;">&lt;Values&gt;true<b>trusted</b></h1>
page: <h1 data-count="2" title="&lt;Values&gt;">&lt;Values&gt;true<b>trusted</b></h1>
<button checked="checked" empty="" title="&lt;&amp;&quot;&#39;">2 -1.5 false</button>
renders: 2
```

The [model/declaration](../crates/razor-slices/examples/support/async_values.rs)
mixes encoded owned text, a numeric attribute, a body boolean and explicit
trusted HTML before the first application await. The
[executable](../crates/razor-slices/examples/async_values.rs) observes this entire
prefix through an 8-byte duplex buffer, checks unchanged completion count,
then notifies later work; it does this twice on the same template. The suffix
verifies true/false/Option/empty-string attributes and body numeric/bool output.
No sleeps are used. The same declaration is exercised over a real local HTTP/1
connection by the Axum suite without changing the framework or original sample.

## Execution, errors and ownership

Construction is lazy. Rust statements, futures, branches and iterator
expressions execute on each render in source order; only selected branches
execute, though all branches are type checked. Borrowed model lifetimes
cannot escape, and owned fields/future fields cannot be consumed through the
shared model binding. Render-local futures and values held across suspension
must satisfy the existing `Send` contract; models must be `Sync`.

Each generated writer operation awaits backpressure and propagates its
original failure with `?`. Application `io::Error` and `RenderError` values
use the existing `AsyncRenderError` conversions (an application I/O error
therefore also uses the `Write` variant); callers can instead return an
`AsyncRenderError` directly. There is no fallback, retry, source replacement,
suffix execution or final flush after failure. Accepted bytes are not rolled back.

`render_into` performs only declared flushes. Child/section insertion does
not finalize output; `render_incremental` adds one final flush after success.
Dropping a pending render drops its application awaits and stops subsequent
work, without closing the caller-owned writer. It cannot undo accepted
prefixes or cancel independently spawned application work. The existing
[runtime limitations](async-rendering.md) remain in effect.

## Upstream grounding and scope

At pinned upstream
[`552937e5f2f21ebcdc73939be014f3d217ffbb22`](https://github.com/DamianEdwards/RazorSlices/tree/552937e5f2f21ebcdc73939be014f3d217ffbb22):

- [`RazorSlice<TModel>`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSliceOfTModel.cs)
  provides a required strongly typed model.
- [`IRazorSliceProxy<TModel>`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/IRazorSliceProxy.cs)
  and the [proxy generator](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/SourceGenerator/RazorSliceProxyGenerator.cs)
  generate typed model factories; [_TodoRow](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/samples/WebApp/Slices/_TodoRow.cshtml)
  and [Todos](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/samples/WebApp/Slices/Todos.cshtml)
  exercise typed fields, iteration and awaited child rendering.
- [`ExecuteAsync` / `FlushAsync`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.cs)
  execute application awaits against the active destination; [Streaming.cshtml](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/samples/WebApp/Slices/Streaming.cshtml)
  explicitly flushes before later awaits.
- [`PipeWriterHtmlExtensions`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/PipeWriterHtmlExtensions.cs)
  supplies primitive formatting and encoded text fast paths; this port reuses
  its existing Rust representations and encoder rather than duplicating policy.
- [`RazorSlice.HtmlAttributes`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.HtmlAttributes.cs)
  suppresses null/false single values with empty prefixes and substitutes the
  attribute name for true. The Rust adapter preserves this existing bounded
  single-value subset, not the compound attribute state machine.

This preserves those typed creation and ordered async execution capabilities
within the [accepted inline proc-macro mechanism](https://github.com/davidfowl/razor-rust/issues/3#issuecomment-6132090066).
The particular declaration/model/async syntax is delegated reversible
implementation discretion, not explicit maintainer approval. Rust reuse
re-executes one shared-borrow renderer rather than pooling/disposal of C#
slice instances. It does not claim identical encoder policy or C# lifecycle.

**Known gaps:** external-file/Razor parsing is not introduced. Async declarative
layouts and inherited section context remain [#47](https://github.com/davidfowl/razor-rust/issues/47)
and [#36](https://github.com/davidfowl/razor-rust/issues/36); explicit associations
use [async-sections.md](async-sections.md). Framework response lifecycle stays
[#52](https://github.com/davidfowl/razor-rust/issues/52)/[#56](https://github.com/davidfowl/razor-rust/issues/56).
Cancellation tokens, threshold flushes, local-only writers and async trait
objects remain #11. Historical ADR reconciliation stays #28/PR #29.
No epic or whole-port landing is claimed.
