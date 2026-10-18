# Synchronous render-local code blocks

`html!` accepts `@{ Rust statements }` anywhere a template body is accepted.
This reuses the existing `async_template!` statement parsing and code generation,
not a second execution engine or a new runtime policy.

```rust
use razor_slices::{html, Slice};

let values = vec![1, 2];
let page = html! {
    @{ let mut total = 0; }
    "<ul>"
    for value in &values {
        @{ total += *value; }
        "<li>" {value} "</li>"
    }
    "</ul><p>" {total} "</p>"
};
for _ in 0..2 {
    assert_eq!(
        page.render_to_string().unwrap(),
        "<ul><li>1</li><li>2</li></ul><p>3</p>"
    );
}
```

Blocks execute only during rendering. Bindings are visible to subsequent parts
of the containing template scope; the `@{ ... }` delimiter does not create a
separate Rust lexical scope. Branch/loop scopes and inner Rust blocks retain
ordinary Rust scoping. Mutable locals can accumulate values across later code
blocks without mutably borrowing captures. Local values are recreated on each
render; effects on captured models persist and are not rolled back on failure.

A semicolon-less tail must return unit, matching the existing async block
contract. Preceding bindings remain visible after a unit tail. Integer,
`Result` and future tails are rejected rather than silently discarded. Use `?`
to propagate compatible errors explicitly or a semicolon when intentionally
discarding an expression. Code can allocate or perform side effects just as
ordinary application Rust can; the macro adds no full-page buffering.

The renderer remains `Fn`: blocks cannot consume owned captures or allow
borrowed captures to outlive their referents. `@await`, `@flush`, `@render`,
`@section` and Rust `.await` do not become legal in synchronous templates.
Use the existing synchronous value/partial contracts or `async_template!` for
async rendering. There is no new flush, cancellation, disposal or encoder policy.
Render-local values use ordinary Rust `Drop`, not C# disposal hooks.

## Executed Razor reference

The dedicated `tests/sync-code-blocks` project uses the .NET 10 Razor SDK to
compile an actual `Local.cshtml`, inheriting unchanged runtime source from
[DamianEdwards/RazorSlices at 552937e5f2f21ebcdc73939be014f3d217ffbb22](https://github.com/DamianEdwards/RazorSlices/tree/552937e5f2f21ebcdc73939be014f3d217ffbb22),
VersionPrefix 0.12.1. The fixture puts locals and a `using` lease in a Razor
code block, mutates a total within `foreach`, and renders the bound locals
afterward. The program loads the SDK's compiled item, rather than substituting
a handwritten `ExecuteAsync` implementation.

The reference was executed with SDK 10.0.402 in Debug and Release. Its three
success rows require two same-instance renders through both actual TextWriter
and PipeWriter destinations to agree, including output, effect counts and one
render-local cleanup per attempt. The remaining 37 rows measure every ASCII
TextWriter failure prefix, original exception identity, the exact count of
entered application effects and render-local cleanup.

Generated Rust sync text/byte and existing async paths match the same 40-row
golden, including accepted prefixes, entered effects and cleanup. Additional
Rust tests cover local ownership/shadowing and hygiene, repeated rendering,
explicit `?` errors, Unicode failure prefixes, final-flush errors, held writes
with cancellation/drop, rejected tail/type/scope/capture combinations and
renamed runtime dependencies.

This is bounded common-output/local-state evidence. It does not pin the Razor
compiler independently of the recorded SDK, exercise RazorSlices' separate proxy
source generator, copy C# managed disposal/pooling, or establish broad encoder,
request-lifecycle or cancellation equivalence. The Rust input language remains
the accepted inline Rust DSL.

From the root, with .NET 10 and the clean pinned reference checkout:

```sh
mkdir -p target
dotnet run --project tests/sync-code-blocks/SyncCodeBlocks.csproj \
  -p:RazorSlicesSource="$(cd ../RazorSlices-parity-reference && pwd)" \
  --verbosity quiet > target/csharp-sync-code-blocks.txt
diff -u crates/razor-slices/tests/fixtures/sync-code-blocks.txt target/csharp-sync-code-blocks.txt
cargo test --locked -p razor-slices-macros
cargo test --locked -p razor-slices --test sync_code_blocks --test sync_code_blocks_negative --test compiler_code_blocks
cargo test --manifest-path crates/razor-slices/tests/renamed/Cargo.toml
```

The C# project rejects a different commit or dirty reference runtime source.
The code-block parser branch is placed before conditional parsing, independently
of the separate `while` capability in PR #86; that branch is not a dependency.
