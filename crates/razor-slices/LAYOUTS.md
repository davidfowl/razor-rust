# Layout around a typed todo body

Focused synchronous layout/body milestone
[#34](https://github.com/davidfowl/razor-rust/issues/34), delivered separately in
PR #35. It explicitly stacks on **Ready/unmerged PR #33**, which depends on
**Ready/unmerged PR #31**. These dependencies are not yet on main; integration
order is #31, #33, then this delivery. Neither delivered dependency branch is
changed here.

From this branch's repository root:

```sh
cargo run -p razor-slices --example layout
```

Actual stdout, checked against `tests/fixtures/layout.html`:

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>Todos &lt;today&gt; &amp; &quot;tomorrow&quot; &#39;世界&#39;</title>
</head>
<body>
<main>
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
</main>
<footer>Layout footer.</footer>
</body>
</html>
```

The entry point is `examples/layout.rs`. `examples/support/layout.rs` defines a
typed `LayoutModel`, `PageLayout` and `todos_document` factory. It reuses the
existing `todos_page`/`todo_row` factories from `examples/support/todos.rs`, not a
second model/loop/partial implementation. The factory associates the body with
its layout using `with_layout(todos_page(todos), PageLayout { model })`.

## Contract

`Layout::render_layout(&self, body: &dyn Slice, writer: &mut dyn io::Write)`
receives the body and output destination. `with_layout(body, layout)` owns
those arguments (or references) and returns a reusable `Slice`; constructing it
does not execute or buffer either template. On each render the layout runs
first, rendering the body only where the layout inserts it. The sample uses
existing `partial(body)` inside `html!`, then writes its tail after successful
body rendering. There is no second body/raw-HTML implementation.

The typed title is ordinary escaped model text. The body's markup is already
produced by a slice and is not re-encoded; its model values remain escaped
exactly once. A trusted `HtmlString` footer stays raw. Layouts and bodies can
be borrowed, owned or boxed trait objects; references must outlive the composed
document. Nested explicit layouts work without implicit layout discovery.
Layouts control body insertion count/order; this helper does not impose an
exactly-once rule on arbitrary implementations.

The existing `InlineSlice` and `Slice::render_to_writer` bridges perform the
composition. Original byte/destination/value errors retain their kind/source
through nested generated layouts and partials. Implementations use `?` to stop
on the first failure: a head failure skips the body, a body failure skips the
tail, and a tail failure remains an error. Output may be partial. There is no
implicit flush, cache, fallback output, hidden model copying or new dependency.
Text-facing render/string methods keep the existing `fmt::Error`/`RenderError`
boundary. The existing async bridge still buffers the complete document.

## Upstream grounding and limits

Grounded at upstream commit
[`552937e5f2f21ebcdc73939be014f3d217ffbb22`](https://github.com/DamianEdwards/RazorSlices/tree/552937e5f2f21ebcdc73939be014f3d217ffbb22):
[`RazorSlice.RenderViaLayout`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.cs)
creates the layout and sets its content slice before rendering the layout;
[`RazorLayoutSlice.RenderBodyAsync`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorLayoutSlice.cs)
delegates to child rendering at the insertion point, returning empty HTML
rather than buffering a body string;
[`_Layout.cshtml`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/samples/WebApp/Slices/_Layout.cshtml)
escapes a typed title and renders body markup inside the page structure.
This sample preserves that synchronous rendering direction/order.

The association here is an explicit Rust composition constructor, not generated
`IUsesLayout` metadata or automatic layout discovery. A composed document is a
normal `Slice`: explicitly passing it to `partial` includes that layout.
Upstream child rendering disables a child's declared layout. For equivalent
bare-body partial usage here, pass the body before `with_layout`; automatic
declared-layout bypass has focused follow-up
[#36](https://github.com/davidfowl/razor-rust/issues/36) under #4.
There is no optional missing body: use an empty slice deliberately.

Named sections, declarative model/layout association, inherited layout context
and async execution remain in [#4](https://github.com/davidfowl/razor-rust/issues/4).
No Razor `@section` syntax is introduced (upstream does not support it).
Generated model declarations/control-flow/conditional attributes remain
[#3](https://github.com/davidfowl/razor-rust/issues/3); async/context-aware partials
[#5](https://github.com/davidfowl/razor-rust/issues/5); web integration
[#6](https://github.com/davidfowl/razor-rust/issues/6); true asynchronous streaming
[#11](https://github.com/davidfowl/razor-rust/issues/11).
The sample omits upstream CSS, external links and section-dependent footer
content. Dates remain deterministic optional text, not DateOnly/culture
formatting. Rust's reusable lifecycle and encoder are not identical to upstream
disposal/configurable .NET HtmlEncoder behavior.

The constructor/trait are owner-delegated coordinator implementation choices
recorded before coding in #34, not claimed explicit maintainer API acceptance.
Shared ADR/index/README ownership remains #28/PR #29; only sample-specific
documentation changes here.
