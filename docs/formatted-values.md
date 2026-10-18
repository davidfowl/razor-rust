# Explicit synchronous per-value formatting

This bounded [#2](https://github.com/davidfowl/razor-rust/issues/2) /
[#3](https://github.com/davidfowl/razor-rust/issues/3) slice adds
`WriteHtmlValue` for `std::fmt::Arguments`. It does not add a blanket `Display`
fallback, change primitive defaults or introduce new template grammar.

```rust
use razor_slices::{html, Slice};

let value = 42;
let page = html! {
    "<p>" {format_args!("{value:04}")} "</p>"
};
assert_eq!(page.render_to_string().unwrap(), "<p>0042</p>");
```

Construct arguments at the interpolation point; Rust's ordinary formatting
argument temporary/borrow rules apply. Do not return borrowed formatting
arguments from a helper or store temporary argument arrays beyond their lifetime.
The reusable template evaluates formatting again each time it renders.
References and `Option` use their existing delegation contracts.

## Encoding, streaming and errors

Rust's formatting traits handle width, precision, numeric base, alignment and
custom `Display`/`Debug` implementations. Every emitted text chunk passes through
the existing five-character HTML encoder. This includes format-string literals,
fill characters and formatted `HtmlString`: formatting explicitly turns its
representation into encoded text, rather than preserving its direct raw-HTML
interpolation contract.

The adapter writes each chunk before the formatter continues; it does not
collect an intermediate formatted `String` or retry the formatter with a larger
buffer. Its own storage is constant-size and it never flushes. A user's formatter
or destination may allocate; this is not a global allocation-free guarantee.

The first destination `io::Error`, including its kind and original payload,
survives the `fmt::Write` bridge. The adapter becomes terminal after that error,
even if a custom formatter incorrectly ignores `fmt::Error` and returns success.
A formatter returning `fmt::Error` without a destination error becomes an
explicit `io::ErrorKind::Other` carrying `fmt::Error`; a template suffix is not
rendered. Partial output is not rolled back. As before, the `Slice::render`
text-only interface can represent failure only as `fmt::Error`; use
`render_to_writer`/`InlineSlice::write_to` to retain I/O error identity.

This is synchronous support. The existing `render_async` buffered bridge can
render such a synchronous slice before its async destination write.
`fmt::Arguments` does **not** implement `AsyncWriteHtmlValue`: it is not `Sync`,
and synchronous Rust formatting cannot await destination backpressure. For
incremental async templates, use an explicit supported string or a custom
`AsyncWriteHtmlValue` implementation. There is no new conditional-attribute
contract or implicit async buffering.

## Exact pinned reference and baseline

The authority is unchanged
[RazorSlices 0.12.1, commit `552937e5f2f21ebcdc73939be014f3d217ffbb22`](https://github.com/DamianEdwards/RazorSlices/tree/552937e5f2f21ebcdc73939be014f3d217ffbb22).
The dedicated C# harness compiles its runtime source and rejects another commit
or dirty source.

[`RazorSlice.Write.cs`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.Write.cs)
provides explicit `WriteSpanFormattable`/`WriteUtf8SpanFormattable`, forwarding
format/provider to the value and encoding its result.
[`RazorSlice.Formattables.cs`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.Formattables.cs)
includes explicit numeric format/provider overloads. The writer extensions use
stack buffers and grow pooled buffers when needed. Rust instead streams
`fmt::Arguments` through its existing encoder; callback counts, buffer strategy
and formatting languages are not claimed equivalent.

Before this change, on Rust main `5488dc3a61fb3afbdc5215cba63c42e36a0a2126`,
the example above fails E0277 because `Arguments<'_>` lacks `WriteHtmlValue`.
Using `format!` to allocate a string already worked; the missing capability is
explicit standard formatting arguments streamed without that intermediate string.
No direct custom-value fallback was inferred from this compile failure.

The [32-line fixture](../crates/razor-slices/tests/fixtures/formatted-values.txt)
contains 30 common rows, matching C# TextWriter/PipeWriter and Debug/Release:
zero padding, signed padding, hexadecimal, fixed precision, escaped custom text,
empty output, formatted markup, an 8,205-byte long value and repeated rendering,
plus original destination errors at all 21 byte offsets of
`P&lt;custom&amp;&gt;S`. Both harnesses assert long output exactly; the fixture
summarizes it by length and FNV-1a hash.
Rust-only checks additionally prove chunk visibility during formatting, Unicode,
all five escapes, trust boundaries, options/references, custom formatter failure
and sticky destination errors. Doctests retain rejection of unwrapped custom
`Display` and incremental async formatting arguments.

The two `csharp/` rows are deliberately reference-only: `fr-FR` produces
`12,50`, and an offered `ICustomFormatter` is not dispatched by the span helper.
The custom span value checks exact provider identity and format forwarding.
Rust formatting uses Rust traits/syntax, **not** .NET culture, format strings,
`IFormatProvider` or `ICustomFormatter`. No date, locale, floating-point-general
representation, encoder-policy or full #2/#3 parity is established.

## Reproduce

From the repository root, with the pinned checkout at `$RazorSlicesSource`:

```sh
dotnet run --project tests/formatted-values --configuration Debug \
  -p:RazorSlicesSource="$RazorSlicesSource" > /tmp/formatted-debug.txt
dotnet run --project tests/formatted-values --configuration Release \
  -p:RazorSlicesSource="$RazorSlicesSource" > /tmp/formatted-release.txt
diff -u crates/razor-slices/tests/fixtures/formatted-values.txt /tmp/formatted-debug.txt
diff -u /tmp/formatted-debug.txt /tmp/formatted-release.txt

cargo test -p razor-slices --test formatted_values --test inline --test optional_values
cargo test -p razor-slices --all-features --test formatted_values --test inline --test optional_values
cargo +1.75 test -p razor-slices --test formatted_values --test inline --test optional_values
cargo +1.75 test -p razor-slices --all-features --test formatted_values --test inline --test optional_values
cargo test -p razor-slices --doc inline::formatted
```

No new dependency, public wrapper/export, macro, primitive/byte encoder or
incremental writer change is required. Implementation is isolated in the existing
inline rendering module's private `formatted` submodule.
