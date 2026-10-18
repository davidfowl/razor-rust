# Streamed template match arms

Both `html!` and `async_template!` accept a template-level Rust-style `match`.
Each arm contains a braced template body, not an ordinary value expression:

```rust
use razor_slices::{html, Slice};

let model = Some(String::from("<&>"));
let page = html! {
    match &model {
        Some(value) if !value.is_empty() => { "<b>" {value} "</b>" },
        _ => { "empty" }
    }
};
for _ in 0..2 {
    assert_eq!(page.render_to_string().unwrap(), "<b>&lt;&amp;&gt;</b>");
}
```

An interpolated expression such as `{ match code { 0 => "zero", _ => "other" } }`
already works and remains unchanged. That chooses a single value satisfying the
value-writing contract. Template arms instead stream heterogeneous literals,
values, partials and nested bodies directly to the existing writer. Async arms
also accept existing async directives. No intermediate HTML string or homogeneous
child-template return type is needed.

The scrutinee is evaluated once each time rendering reaches the match. Patterns
and guards follow source order; unselected bodies are not executed. Rust checks
exhaustiveness (guards alone cannot establish it), pattern/guard types, binding
scope, capture ownership and async `Send` requirements. Arm bindings do not escape
the arm. Repeated sync renders retain the existing reusable `Fn` contract: borrow
owned captures rather than move them out.

Patterns support native bindings, destructuring, ranges, `@` bindings, or-patterns
and leading `|`. Commas after braced arms are optional, including the last arm.
Empty bodies are allowed; an empty match still needs an uninhabited scrutinee as
required by Rust. Arm attributes and unbraced value arms are not DSL forms.
There is no C# `case`, `default`, fallthrough, `goto`, or new `break` grammar;
use Rust patterns and `_`. Available bodies are exactly the existing grammar
on the target branch; separate `while` and sync code-block PRs are not dependencies.

Output, value evaluation and errors retain their existing order. A failed prefix
before the match prevents even scrutinee evaluation; failure within an arm stops
later work. Async backpressure retains the selected bindings/future rather than
reselecting, and dropping that future releases ordinary Rust locals. There is no
new buffering, flush/cancellation, encoding or managed lifecycle policy.

## Executed reference and limits

The dedicated `tests/template-match` project uses the Razor SDK to compile actual
`Switch.cshtml` against unchanged
[RazorSlices 0.12.1 source at 552937e5f2f21ebcdc73939be014f3d217ffbb22](https://github.com/DamianEdwards/RazorSlices/tree/552937e5f2f21ebcdc73939be014f3d217ffbb22).
It rejects a different commit or dirty reference runtime source. The SDK compiled
item is loaded directly, not replaced with a handwritten `ExecuteAsync`.

SDK 10.0.402 Debug and Release produce the committed 23-row golden:
six success cases, each repeated on the same instance through actual TextWriter
and PipeWriter destinations, plus all 17 ASCII TextWriter failure prefixes for
the guarded/bound case with original exception identity and ordered application
effects. Cases cover grouped labels, a guarded pattern binding, skipped guards,
default selection and no fallthrough. Rust sync text/byte and existing async
paths agree with those outputs and traces.

Additional Rust acceptance covers heterogeneous/nested bodies, binding/writer
hygiene, changing model state between renders, empty arms, owned borrowed
captures, selected async awaits/flushes, backpressure/completion/drop and original
scrutinee/selected-await errors. Ten standalone doctests include a positive case
and nine native compile-rejection contracts. Renamed-runtime tests exercise both
entry points, including a colliding generic writer-type name.

This is bounded common control-flow evidence, not Razor/C# grammar or complete
compiler parity. Rust `match` exhaustiveness, or-patterns and ownership are Rust
contracts, not claims about equivalent C# syntax. The runtime is commit-pinned;
the compiler is identified by SDK version, not an independently pinned compiler
commit. RazorSlices' separate proxy source generator, broad encoder equivalence,
managed disposal/pooling, request lifecycle and cancellation parity are not tested
by this harness.

With .NET 10 and a clean reference checkout at the pin, from the repository root:

```sh
mkdir -p target
dotnet run --project tests/template-match/TemplateMatch.csproj \
  -p:RazorSlicesSource="$(cd ../RazorSlices-parity-reference && pwd)" \
  --verbosity quiet > target/csharp-template-match.txt
diff -u crates/razor-slices/tests/fixtures/template-match.txt target/csharp-template-match.txt
cargo test --locked -p razor-slices-macros
cargo test --locked -p razor-slices --test template_match --test template_match_negative
cargo test --manifest-path crates/razor-slices/tests/renamed/Cargo.toml --test template_match
```
