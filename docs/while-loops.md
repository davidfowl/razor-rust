# Condition-controlled template loops

Both `html!` and `async_template!` accept recursively nested `while` and
`while let` bodies, in addition to the existing `if` and `for` grammar:

```text
while RustCondition { template }
while let RustPattern = RustExpression { template }
```

Conditions execute only when rendering, before every iteration, including the
final false/absent result. Pattern bindings are local to the selected body.
Zero-iteration bodies still undergo Rust type checking but do not evaluate
values or application work. Parenthesize struct literals in conditions, just
as with ordinary Rust control flow.

```rust
use razor_slices::async_template;

async_template! {
    struct Countdown { model: usize } => {
        @{ let mut remaining = *model; }
        while remaining > 0 {
            "<span>" {remaining} "</span>"
            @{ remaining -= 1; }
        }
    }
}
let _ = Countdown::new(3);
```

Render-local state is recreated on each asynchronous render. External model
effects persist as before: the runtime does not reset an iterator or roll back
an advanced condition after failure/cancellation. The synchronous template is
still a reusable `Fn`; use interior mutability or borrowed iterator state
rather than moving owned captures out of it.

Nested async loops support existing code blocks, awaited work, children,
sections and explicit flushes at their normal insertion points. A pending await
or destination write stops the iteration before later condition checks.
The first error exits the render with the original destination error; scoped
cancellation remains observable and dropping a render stops further polling.
Accepted output and application effects are not rolled back.

Loop iterations are **not** slice-completion boundaries and do not add flushes.
Completed child templates retain their existing inclusive 16,384-byte checks;
explicit and root-final flushing retain their existing contracts. No artificial
yield or runtime dependency is introduced. A nonterminating loop with no
pending operations can monopolize a poll, just like ordinary handwritten Rust;
applications remain responsible for termination and cooperative work.

No new standalone `match`, labeled-loop or DSL `break`/`continue` syntax is
added. Rust expressions/code blocks retain their ordinary Rust semantics.

## Bounded reference evidence

The reference remains
[DamianEdwards/RazorSlices at 552937e5f2f21ebcdc73939be014f3d217ffbb22](https://github.com/DamianEdwards/RazorSlices/tree/552937e5f2f21ebcdc73939be014f3d217ffbb22),
VersionPrefix 0.12.1. The dedicated `tests/while-loops` project compiles its
unchanged runtime source and executes native C# condition-controlled loops in
handwritten `RazorSlice.ExecuteAsync` overrides. It does not invoke the Razor
source generator or establish `.cshtml` parser parity.

Five shared golden rows compare exact common-ASCII output for boolean/binding
loops with empty/nonempty input and nested conditionals/iteration, on actual
C# TextWriter/PipeWriter destinations and Rust generated sync text/byte/async
paths. Both implementations assert condition counts and body advancement.
C# additionally holds an await inside the first iteration and checks each
ASCII writer-error prefix and original exception identity on TextWriter.
Rust matches those prefixes/condition counts on byte sinks, and separately
checks original final-flush errors, repeated rendering, render-local state,
pending awaits/writes, scoped cancellation/drop, directives, renamed dependency
hygiene, malformed/type/scope/capture rejection and native/fallback
16,383/16,384/16,385-byte completion boundaries.

This is a capability mapping to the accepted inline-Rust mechanism under #3,
not C# syntax parsing, broad encoder equivalence or full cancellation/lifecycle
equivalence. Existing Rust runtime defaults and APIs are unchanged.

With .NET 10 and the clean pinned reference checkout, run from the root:

```sh
mkdir -p target
dotnet run --project tests/while-loops/WhileLoops.csproj \
  -p:RazorSlicesSource="$(cd ../RazorSlices-parity-reference && pwd)" \
  --verbosity quiet > target/csharp-while-loops.txt
diff -u crates/razor-slices/tests/fixtures/while-loops.txt target/csharp-while-loops.txt
cargo test --locked -p razor-slices-macros
cargo test --locked -p razor-slices --test while_loops
cargo test --manifest-path crates/razor-slices/tests/renamed/Cargo.toml
```

The harness rejects a different source commit or modified runtime source.
