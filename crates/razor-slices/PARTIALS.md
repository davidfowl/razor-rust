# Typed todo-list and reusable partials

This is the focused synchronous child-rendering milestone in
[#32](https://github.com/davidfowl/razor-rust/issues/32), under
[#5](https://github.com/davidfowl/razor-rust/issues/5). It uses the inline-template
implementation from **Ready/unmerged PR #31**, not functionality already on main.
The separate delivery PR #33 targets main and explicitly includes PR #31's
base commits; it should be integrated after that dependency.

Run from this branch's repository root:

```sh
cargo run -p razor-slices --example todos
```

Actual output, also checked against `tests/fixtures/todos.html`:

```html
<h1>Todos (2)</h1>
<table>
<tr><th>Id</th><th>Title</th><th>Due by</th><th>Status</th></tr>
<tr>
  <td>1</td>
  <td>Wash &lt;dishes&gt; &amp; &quot;cups&quot;</td>
  <td></td>
  <td>Completed</td>
</tr>
<tr>
  <td>2</td>
  <td>Call &#39;Grandma&#39; 世界</td>
  <td>2026-10-14</td>
  <td>Incomplete</td>
</tr>
</table>
<footer>Rendered with reusable partials.</footer>
```

`examples/support/todos.rs` defines a typed `Todo`, `todo_row(&Todo) -> impl Slice`
and `todos_page(&[Todo]) -> impl Slice`. The row accesses typed fields, renders
missing due dates as empty text, and selects Completed/Incomplete with a normal
Rust `if` expression. The page uses a normal Rust loop inside `InlineSlice::new`
and renders each row with `partial(todo_row(model)).write_html_to(writer)`.
The entry point at `examples/todos.rs` interpolates the whole page with
`html! { {partial(&page)} ... }`, includes an explicit trusted `HtmlString`
footer, and asserts repeated renders agree before writing to stdout.

## Partial contract

`partial(child)` creates a `Partial<S>: WriteHtmlValue`, not a pre-rendered
`HtmlString`. It writes the child directly to the parent's byte destination
without buffering, caching, flushing or escaping the child's markup again.
Child values still use normal HTML encoding, so `<dishes>` becomes
`&lt;dishes&gt;` exactly once; explicit trusted `HtmlString` remains raw.
Arbitrary strings cannot be treated as partials, and a bare slice cannot be
interpolated without the adapter. Handwritten slices, inline slices, nested
partials, references and `Box<dyn Slice>` all work.

Use `partial(&child)` to render a captured child repeatedly. To own a child
wrapper, bind `let child = partial(make_child());` before capturing it in `html!`.
Calling `partial(child)` inside a reusable render expression would move out of
its capture, so Rust rejects it. Model references and borrowed children cannot
outlive their referents. The sample uses ordinary typed Rust factories, which
reject incorrect model arguments at compile time, not a generated model DSL.

`Slice::render_to_writer` is the object-safe byte-rendering seam for composition.
Its default adapts `fmt::Write` to `io::Write` incrementally. It retains the first
destination `io::Error`, including its original kind/source, even if a handwritten
text renderer ignores a failed write; later writes remain failed.
An independent handwritten `fmt::Error` becomes `io::ErrorKind::Other` with
that error as source. `InlineSlice` overrides this method to retain original
value/destination I/O errors through nested partials, and reference/box
implementations forward the override. A byte renderer should stop at its first
error, and may leave partial output. No implicit flush occurs.

The text-facing `Slice::render` and its string/byte convenience methods still
have the existing `fmt::Error`/`RenderError` boundary; use `render_to_writer`
when detailed I/O errors matter. The existing `render_async` buffers the whole
rendered result, including composed children; it does not become true incremental
async rendering. Byte-native custom values remain responsible for valid UTF-8;
text rendering validates split/malformed UTF-8 through the existing bridge.

## Upstream parity and limits

Grounded in upstream Razor Slices at
[`552937e5f2f21ebcdc73939be014f3d217ffbb22`](https://github.com/DamianEdwards/RazorSlices/tree/552937e5f2f21ebcdc73939be014f3d217ffbb22):
[`Todos.cshtml`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/samples/WebApp/Slices/Todos.cshtml)
loops over a typed model and renders `_TodoRow.Create(todo)`;
[`_TodoRow.cshtml`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/samples/WebApp/Slices/_TodoRow.cshtml)
accesses model fields and chooses status text;
[`RazorSlice.Partials.cs`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.Partials.cs)
renders a child into the parent writer and returns empty HTML, rather than
returning a buffered string to encode. This adapter preserves that rendering
direction synchronously, while keeping Rust's reusable `&self` contract.

This sample deliberately omits the upstream layout, CSS/conditional class
attribute and title hyperlink. Due dates are deterministic optional text,
not a port of DateOnly/culture formatting. Models are borrowed Rust values,
not generated C#-style proxy definitions. It does not copy the upstream async
execution/disposal lifecycle or propagate DI, encoder, cancellation or layout
context. Encoding follows the existing Rust encoder, not byte-for-byte .NET
HtmlEncoder parity.

Owner-delegated implementation choices were recorded before coding in #32;
no new DSL or implicit raw-output/type fallback was introduced, and this is not
a claim of explicit maintainer approval of every API detail. Remaining work:
model declarations/dedicated control-flow and conditional attributes
[#3](https://github.com/davidfowl/razor-rust/issues/3), layouts/sections
[#4](https://github.com/davidfowl/razor-rust/issues/4), async/context-aware partials
[#5](https://github.com/davidfowl/razor-rust/issues/5), web integration
[#6](https://github.com/davidfowl/razor-rust/issues/6), and true async streaming
[#11](https://github.com/davidfowl/razor-rust/issues/11). This does not complete
those epics or the port.
