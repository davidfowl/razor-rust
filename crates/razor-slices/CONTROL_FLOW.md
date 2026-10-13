# Nested template control flow

Run from the repository root:

```sh
mkdir -p target
cargo run -p razor-slices --example control_flow > target/control-flow-output.html
diff -u crates/razor-slices/tests/fixtures/control_flow.html target/control-flow-output.html
```

The executable asserts repeated rendering and prints the exact escaped output
in [the fixture](tests/fixtures/control_flow.html). The
[typed model-list factory](examples/support/control_flow.rs) nests loops,
completed/due/unscheduled branches, an empty-list branch and a reusable partial,
without handwritten `InlineSlice` factories.

```rust
use razor_slices::{html, Slice};
let values = vec!["<&>", ""];
let page = html! {
    "<ul>"
    for value in &values {
        if value.is_empty() { "<li>empty</li>" }
        else { "<li>" {value} "</li>" }
    }
    "</ul>"
};
assert_eq!(
    page.render_to_string().unwrap(),
    "<ul><li>&lt;&amp;&gt;</li><li>empty</li></ul>"
);
```

## Grammar and ownership

Each template body accepts the existing quoted literals and `{ expression }`
interpolations, plus recursively nested:

```text
if RustCondition { template }
if RustCondition { template } else { template }
if RustCondition { template } else if RustCondition { template } else { template }
for RustPattern in RustIteratorExpression { template }
```

`if let` bindings, tuple/destructuring loop patterns, empty bodies and arbitrary
nesting work through Rust/syn parsing. Headers use Rust's ordinary control-flow
expression boundary: parenthesize struct literals, e.g.
`if (Options { enabled: true }).enabled { ... }`. A dangling `else`, missing
`in`/body, unsupported tokens or invalid nested content is a compile error,
never silently emitted text. `while`, `match`, DSL `break`/`continue`, declarations,
Razor directives and asynchronous expressions are not introduced.

Conditions and iterator expressions execute on each render, in source order.
Unselected bodies and empty loops do not evaluate interpolations; all branches
are nevertheless type checked. Loop bindings are scoped to their body.
Literals remain raw and strings remain escaped once; `HtmlString` and explicit
`partial(child)` retain their existing behavior. The writer and renamed runtime
path are hygienic even when loop/user bindings use `__razor_writer`.

The renderer is still `move` + `Fn`: iterate `&owned_values` or
`owned_values.iter()` to retain an owned captured collection across renders.
Iterating a captured owned `Vec` by value is rejected rather than making the
template one-shot. Captured borrowed models cannot escape their lifetime;
mutable captured borrows are not supported. This is a typed Rust factory, not a
new model declaration DSL. For separately declared typed async templates using
this recursive body grammar, see [async-templates.md](../../docs/async-templates.md);
the synchronous capture contract described here is unchanged.

Generated branch and loop writes use the same fallible write path as top-level
parts. The first failed literal, value or child stops the render immediately,
including later iterations and suffixes; output already written is not rolled
back. `render_to_writer`/`write_to` preserve the original `io::Error` kind and
source and do not flush. Text rendering retains the existing `fmt::Error` /
`RenderError` boundary. No core runtime change or extra buffering is required.

## Provenance, parity and remaining scope

The owner accepted the inline proc-macro mechanism in
[#3](https://github.com/davidfowl/razor-rust/issues/3#issuecomment-6132090066).
This bounded grammar is an owner-delegated reversible implementation choice in
[#41](https://github.com/davidfowl/razor-rust/issues/41), not a claim that a
maintainer explicitly approved these keywords or the reusable capture contract.
It builds on unmerged #31/#33, not functionality already on main.

Upstream
[Todos.cshtml](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/samples/WebApp/Slices/Todos.cshtml)
iterates typed models with `@foreach` and renders child rows.
[_TodoRow.cshtml](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/samples/WebApp/Slices/_TodoRow.cshtml)
selects completed/incomplete output from typed model fields. This delivers the
same control-flow capability using Rust loop/conditional code generation, not
Razor parsing, C# syntax or byte-identical .NET encoder/lifecycle behavior.
The sample deliberately adds grouping and empty lists to exercise nesting.
Its due dates are deterministic text, not culture-sensitive `DateOnly` values.

Typed async declaration/await syntax is the separate #58 milestone under #3;
advanced syntax remains #3, layouts/sections #4,
async/context-aware partials #5, HTTP integration #6, streaming #11, benchmarks
and broader samples #8. Conditional attribute semantics and upstream CSS/layout
are not implemented here. Shared README/ADR/index and unrelated worker files
remain untouched; no epic is closed by this delivery.

Dedicated integration tests compare the exact fixture on text/byte/direct
writer sinks and cover branch timing, empty loops, reuse, captures, patterns,
raw/partial composition, hygiene and first-error retention/halting. The
standalone `tests/control-flow-negative` doctests cover eight grammar/type/
capture/lifetime rejections and run from the normal workspace test suite.
`tests/renamed/tests/control_flow.rs` exercises nested codegen with a renamed
runtime dependency.
