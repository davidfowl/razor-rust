# UTF-8 byte-buffer body values

`html!` and `async_template!` accept byte slices, fixed-size byte arrays and
`Vec<u8>` as encoded UTF-8 body values. References and optional values retain
their existing delegation: `None` emits nothing; `Some` borrows its payload.
Byte values are not trusted markup and are not rendered as numeric lists.

```rust
use razor_slices::{html, Slice};

let bytes = b"A&<B>\"".to_vec();
let page = html! { "<p>" {bytes} "</p>" };
assert_eq!(page.render_to_string().unwrap(), "<p>A&amp;&lt;B&gt;&quot;</p>");
```

Malformed UTF-8 is decoded with U+FFFD replacement, exactly as
`String::from_utf8_lossy`, then passed through the existing string encoder.
This is an intentional input representation contract, not recovery from an
I/O error. Original destination errors still stop rendering immediately.
There is no full-value decoding allocation: valid UTF-8 runs are borrowed and
replacement characters are static strings. Async writes retain the existing
bounded string scratch, backpressure, cancellation and completion accounting;
byte interpolation never implicitly flushes.

## Pinned C# evidence and boundaries

The reference remains
[DamianEdwards/RazorSlices at 552937e5f2f21ebcdc73939be014f3d217ffbb22](https://github.com/DamianEdwards/RazorSlices/tree/552937e5f2f21ebcdc73939be014f3d217ffbb22),
VersionPrefix 0.12.1. Its `RazorSlice.Write.cs` has encoded byte-array,
`ReadOnlyMemory<byte>` and `ReadOnlySpan<byte>` overloads. The dedicated harness
executes all three on both actual PipeWriter and TextWriter destinations,
requiring byte values to match `Encoding.UTF8.GetString` string rendering.
Eleven cases cover empty/ASCII/escaped values, NUL, Unicode, invalid leading and
continuation bytes, truncation, overlong sequences, surrogates and out-of-range
scalars. All six overload/destination combinations must agree per case.

Only the first three common ASCII cases are shared byte-for-byte Rust/C#
goldens. C# default encoding emits numeric entities for the selected Unicode,
NUL and replacement scalars; Rust deliberately keeps its existing string
encoding policy, including UTF-8 U+FFFD output and `&#39;` for apostrophes.
Replacement decoding parity is not blanket encoder-policy parity.
Conditional byte-buffer attributes and new formatting/encoder APIs are outside
this slice. Other non-byte collections remain unsupported.

From the repository root, with .NET 10 and the clean pinned source checkout:

```sh
mkdir -p target
dotnet run --project tests/parity/Utf8Values.csproj \
  -p:RazorSlicesSource="$(cd ../RazorSlices-parity-reference && pwd)" \
  --verbosity quiet > target/csharp-utf8-values.txt
diff -u tests/parity/fixtures/utf8-values.txt target/csharp-utf8-values.txt
dotnet run --project tests/parity/Utf8Values.csproj \
  -p:RazorSlicesSource="$(cd ../RazorSlices-parity-reference && pwd)" \
  --verbosity quiet -- common > target/csharp-utf8-common.txt
diff -u crates/razor-slices/tests/fixtures/utf8-values.txt target/csharp-utf8-common.txt
cargo test --locked -p razor-slices --test utf8_values
```

The dedicated project explicitly compiles its `.csx` input as regular C# so it
does not introduce another entry point into the existing parity project's
recursive `*.cs` inputs. Its build rejects a different commit or modified
reference runtime source. Both projects can be run independently.

Rust regressions additionally compare every one/two-byte input and deterministic
longer malformed inputs against standard replacement decoding; exercise generated
sync text/byte and async paths, reuse/optional values, every accepted-output
failure offset, pending writes, cancellation/drop, bounded writes, original final
flush errors and native/fallback 16,383/16,384/16,385-byte completion boundaries.
