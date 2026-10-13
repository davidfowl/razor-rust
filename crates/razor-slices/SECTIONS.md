# Lazy named sections

Focused synchronous section milestone [#45](https://github.com/davidfowl/razor-rust/issues/45),
delivered in [PR #46](https://github.com/davidfowl/razor-rust/pull/46).
This branch explicitly stacks on **Ready/unmerged #35**, which includes #33/#31.
Integration order is #31, #33, #35, then #46. These APIs are not yet on main;
no dependency branch is changed here.

## Run a complete page

From this branch's repository root:

```sh
cargo run -p razor-slices --example sections
```

`examples/support/sections.rs` defines a typed `PageModel`, `SectionLayout`
and `todos_document` factory. The existing typed todo body is reused, not
copied. The layout inserts the header before the body and the footer after it;
an absent `optional-scripts` section intentionally produces no output.
Actual stdout is checked against `tests/fixtures/sections.html` locally and
in Actions:

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>Todos &lt;sections&gt; &amp; 世界</title>
</head>
<body>
<header><p>Lazy &lt;header&gt; &amp; &quot;values&quot;</p></header>
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
<footer>Footer &#39;after&#39; the body</footer>
</body>
</html>
```

## Compose and insert sections

```rust
use razor_slices::{html, partial, section, with_layout, with_section, Layout, Slice};
use std::io;

struct Page;
impl Layout for Page {
    fn render_layout(&self, body: &dyn Slice, out: &mut dyn io::Write) -> io::Result<()> {
        html! {
            "<html>" {section(body, "header")}
            "<main>" {partial(body)} "</main>"
            {section(body, "footer")} "</html>"
        }.render_to_writer(out)
    }
}

let body = with_section(
    html! { "<p>Body</p>" },
    "header",
    html! { "<h1>" {"<Title>"} "</h1>" },
)?;
let page = with_layout(body, Page);
assert_eq!(
    page.render_to_string().unwrap(),
    "<html><h1>&lt;Title&gt;</h1><main><p>Body</p></main></html>"
);
# Ok::<(), io::Error>(())
```

`with_section(body, name, content)` returns `io::Result<impl Slice>`, owns the
two slices (or their references) and the name, and never renders at construction.
Ordinary rendering forwards to the body unchanged, including its writer/string/
byte overrides. Named rendering matches the requested name exactly, writes the
section's body directly, or delegates to the wrapped body's sections. Chain
associations for several names. If names repeat, the outermost association
overrides the inner one without executing shadowed content.

`section(body, name)` implements `WriteHtmlValue` for interpolation. The public,
object-safe `Slice::render_section_to_writer(name, writer)` also supports direct
dispatch or handwritten providers. Reference and box blanket implementations
forward section dispatch to the underlying provider. Bind a reference before
using it in several `html!` move captures; models and borrowed section names
must outlive their renderers. Section providers/content must be slices, not
arbitrary strings.

Names are nonempty, case-sensitive Rust strings. No trimming, HTML identifier
validation or Unicode normalization is performed; names are lookup keys, not
emitted attributes. Empty association names fail at construction; empty
insertion names fail when rendered, both with `InvalidInput`. Handwritten
providers must likewise reject empty direct requests. Unknown nonempty sections
write nothing, even with a failing destination, because no write is attempted.
There is no required-section mode or fallback content.

Sections execute at each insertion, in layout order. They are neither eagerly
buffered nor cached, and can be omitted or repeated. The section's template
escapes its model values normally; the insertion does not re-encode its markup.
`HtmlString` retains its existing trusted-HTML meaning. Body and sections use
the same destination with no implicit flush. Original byte/value/destination
errors propagate through nested layouts; `?` stops subsequent sections, body
and tails. Output may already be partial. Text/string convenience methods
retain the existing `fmt::Error`/`RenderError` boundary.

## Upstream behavior and remaining parity

Pinned C# reference:
[`552937e5f2f21ebcdc73939be014f3d217ffbb22`](https://github.com/DamianEdwards/RazorSlices/tree/552937e5f2f21ebcdc73939be014f3d217ffbb22).
[`RazorLayoutSlice.RenderSectionAsync`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorLayoutSlice.cs)
rejects empty names and delegates to its content slice at the insertion point.
[`RazorSlice.ExecuteSectionAsync`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.Layout.cs)
defaults to a completed task without output. Sample overrides such as
[`_LoremLayout.cshtml`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/samples/WebApp/Slices/Lorem/_LoremLayout.cshtml)
use case-sensitive name checks and do nothing when unmatched. This is the
executable optional-missing behavior followed here; a `RenderSectionAsync`
XML comment instead describes a missing-section exception.

Upstream does not support Razor `@section` directive syntax because of its
streaming incompatibility. This delivery introduces no such syntax; explicit
Rust section slices are the synchronous equivalent of handwritten section
overrides. Its Rust constructor/API is an owner-delegated choice, not claimed
explicit maintainer approval.

Each explicit layout dispatches to its own supplied body. A composed
`with_layout` document does not implicitly inherit the body's section provider.
To expose sections to an outer layout, explicitly associate sections on the
inner document; nested sections remain scoped to their own bodies.
[Typed declared layouts](DECLARED-LAYOUTS.md), delivered for
[#36](https://github.com/davidfowl/razor-rust/issues/36), preserve body section
dispatch and bypass declarations when rendering associated section content.
Generated declaration syntax remains in #3.
Inherited/async section context, cancellation and lifecycle parity have focused
follow-up [#47](https://github.com/davidfowl/razor-rust/issues/47) under #4, alongside
[#11](https://github.com/davidfowl/razor-rust/issues/11); the existing async
bridge still buffers the completed synchronous document. This is not generated
async section syntax or end-to-end HTTP streaming.
Rust reuse and the existing encoder remain different from upstream disposal
and configurable .NET `HtmlEncoder`. The full port/parent epic is not complete.

Shared README/ADR ownership remains #28/PR #29. See [LAYOUTS.md](LAYOUTS.md)
for the underlying explicit layout/body contract.
