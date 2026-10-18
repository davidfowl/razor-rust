# Bounded unsigned integer formatting

`format_integer(value: u64, format: IntegerFormat, min_digits: u8)` constructs a
reusable value implementing both `WriteHtmlValue` and `AsyncWriteHtmlValue`.
This is one explicit numeric subset of [#2](https://github.com/davidfowl/razor-rust/issues/2)
and [#11](https://github.com/davidfowl/razor-rust/issues/11), not an arbitrary
formatter bridge or a general .NET formatting API.

```rust
use razor_slices::{async_template, format_integer, html, IntegerFormat, Slice};

let value = format_integer(255, IntegerFormat::UpperHex, 4);
let page = html! { "<code>" {value} "</code>" };
assert_eq!(page.render_to_string().unwrap(), "<code>00FF</code>");

async_template! {
    struct Page { model: u64 } => {
        "<code>" {format_integer(*model, IntegerFormat::UpperHex, 4)} "</code>"
    }
}
```

## Contract and bounds

| Format | Example | Pinned C# common subset |
| --- | --- | --- |
| `IntegerFormat::Decimal` | `42`, minimum 4: `0042` | UInt64 `D4` |
| `IntegerFormat::LowerHex` | `255`, minimum 4: `00ff` | UInt64 `x4` |
| `IntegerFormat::UpperHex` | `255`, minimum 4: `00FF` | UInt64 `X4` |

Minimum digits is inclusive `0..=255`, enforced by the `u8` argument. Smaller
values never truncate significant digits; zero with precision zero still emits
`0`. Padding is always ASCII zero. No sign, prefix, grouping or whitespace is
emitted. `u64::MAX` requires 20 decimal or 16 hexadecimal digits. Smaller
unsigned integers can use `u64::from(value)` explicitly.

The value stores only the number, enum and precision. Each invocation generates
digits once, right-to-left, into a fixed **255-byte stack buffer** shared by the
sync/async implementations. A smaller precision cannot overflow it: there are
at most 20 significant digits, and requested padding is at most 255. There is no
heap allocation, arbitrary formatter callback, output-size-dependent scratch,
format parsing or replay in this adapter. The destination can still allocate.
The complete bounded numeric value fits in one async `write_all`; this is not
hidden buffering of an arbitrary-size formatter result.

Output is only known ASCII digits and hex letters, so the literal write path is
safe without altering the encoder policy. Original destination I/O errors are
propagated. A pending write holds the fixed buffer until completion/drop, without
advancing to the suffix. Scoped cancellation uses the existing writer/root
contract. Accepted prefixes are not rolled back or reissued. The adapter never
flushes; existing child/root completion policy still applies normally.

The model is `Copy`, `Send` and `Sync`, can be borrowed/repeated, and uses existing
reference/`Option` delegation. Plain integer interpolation still emits decimal
without padding. No new attribute contract or macro syntax is introduced.

## Reference, baseline and evidence

The authority is unchanged
[RazorSlices 0.12.1 at `552937e5f2f21ebcdc73939be014f3d217ffbb22`](https://github.com/DamianEdwards/RazorSlices/tree/552937e5f2f21ebcdc73939be014f3d217ffbb22).
The dedicated C# project checks the source commit and runtime cleanliness.
It calls `WriteNumber((ulong?)value, format, CultureInfo.InvariantCulture)`.
The overload in
[`RazorSlice.Formattables.cs`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.Formattables.cs)
delegates through explicit UTF-8 span formatting in
[`RazorSlice.Write.cs`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.Write.cs).
Only the decimal/hex output subset above is compared, not the .NET buffer or
callback strategy.

Before this change, main `92487bf51ceeb970495842b4f0ba776a43554d5d` has no
explicit bounded integer-format adapter for incremental values. A direct async
`255_u64` value writes `255`, failing the requested `00FF` assertion. That is a
missing opt-in capability, **not a primitive-default defect**. Synchronous
`format_args!` already supports formatting; allocating `format!` followed by
string interpolation also already works. Neither supplies this bounded, typed,
nonallocating async value. The existing arbitrary-Arguments design boundary is
unchanged.

The [156-row fixture](../crates/razor-slices/tests/fixtures/integer-formatting.txt)
is reproduced identically in C# Debug/Release and TextWriter/PipeWriter:

- 144 successes: eight values (`0,1,15,16,42,255,256,u64::MAX`), three formats,
  and six precisions (`0,1,4,20,32,255`).
- 12 original-error cases in `P{formatted}S`: all six failing byte offsets of
  precision 4 and six representative precision-255 offsets, including partial
  digits and the trailing suffix boundary.

Each harness checks the exact output/prefix before comparing compact fixture
length/FNV-1a rows. The C# failure sink intercepts both span and array TextWriter
overloads and preserves the original exception reference, including for the
UTF-8 numeric helper's array-write path.

Additional Rust acceptance exercises every precision for eight edge values in
all formats (6,144 combinations, each through both traits), all byte values,
reference/optional/repeated models and unchanged defaults. It checks the original
error at all 257 offsets of the maximum-width document through both rendering
paths. A held-write matrix uses all 257 offsets with resume, scoped drop, legacy
drop and scoped cancellation (1,028 attempts). It checks exact prefixes, no
suffix/final flush on failure/drop/cancellation, `Send` rendering futures,
caller-owned destination lifetime and healthy reuse after every attempt.
These pending-write/drop cases are Rust-only: C# numeric writes are synchronous.

## Reproduce

From the repository root, with the pinned source at `$RazorSlicesSource`:

```sh
dotnet run --project tests/integer-formatting --configuration Debug \
  -p:RazorSlicesSource="$RazorSlicesSource" > /tmp/integer-debug.txt
dotnet run --project tests/integer-formatting --configuration Release \
  -p:RazorSlicesSource="$RazorSlicesSource" > /tmp/integer-release.txt
diff -u crates/razor-slices/tests/fixtures/integer-formatting.txt /tmp/integer-debug.txt
diff -u /tmp/integer-debug.txt /tmp/integer-release.txt

cargo test -p razor-slices --test integer_formatting --test formatted_values \
  --test optional_values --test automatic_flush
cargo test -p razor-slices --all-features --test integer_formatting \
  --test formatted_values --test optional_values --test automatic_flush
cargo +1.75 test -p razor-slices --test integer_formatting --test formatted_values \
  --test optional_values --test automatic_flush
cargo +1.75 test -p razor-slices --all-features --test integer_formatting \
  --test formatted_values --test optional_values --test automatic_flush
cargo test -p razor-slices --doc integer
```

## Explicit limits

Unsigned 64-bit values only. No signed reinterpretation, UInt128 equivalence,
binary/octal/general/scientific formats, arbitrary precision, grouping, signs,
alternate prefixes, dates, culture/providers or `ICustomFormatter`. This does not
add arbitrary `Display`/`fmt::Arguments` async support, weaken `Send`/`Sync`, alter
primitive output or claim full #2/#11 parity. There are no unsafe, blocking,
spawned or formatter-replay bridges.
