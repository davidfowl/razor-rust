# Explicit conditional attributes

This bounded runtime adapter implements single-value, empty-prefix attributes,
not new template grammar or the entire Razor attribute state machine.
The original #43/#44 and #62/#63 delivery notes describe those milestones;
the current integration source includes both sync and async value contracts.

## Pinned upstream contract

Reference revision:
[`552937e5f2f21ebcdc73939be014f3d217ffbb22`](https://github.com/DamianEdwards/RazorSlices/tree/552937e5f2f21ebcdc73939be014f3d217ffbb22).

- [RazorSlice.HtmlAttributes.cs](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.HtmlAttributes.cs):
  `BeginWriteAttribute` delays the prefix for one value; `WriteAttributeValue`
  suppresses null/false with an empty value prefix and replaces true with the
  attribute name; `EndWriteAttribute` suppresses the suffix too.
- [RazorSliceHtmlAttributesTests.cs](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/tests/RazorSlices/RazorSliceHtmlAttributesTests.cs):
  object-typed true produces ` checked="checked"`, false/null produce nothing,
  and empty strings produce ` class=""`.
- [RazorSlice.Write.cs](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.Write.cs):
  `WriteValue` dispatches strings through encoding and `HtmlString` through
  `WriteHtml` without encoding again.

## Implementation boundaries

The original #43/#44 adapter implements `WriteHtmlValue`; this integration
source includes its inline macro consumers and the #62/#63 explicit async
value contracts. Historical source dependencies are recorded in
[the integration guide](../../docs/integration.md).
Character support below is additive; existing value behavior and name
validation remain unchanged.

Attribute names use a conservative ASCII grammar, rejecting malformed
names explicitly before any writing, even for an omitted value. This is a
runtime safety boundary absent from the upstream compiler-generated name path.
It does not sanitize URL schemes or prohibit event-handler attributes.

Compound/prefixed attribute parts, compiler-specific data-* treatment, general
object dispatch and byte-identical .NET encoding/culture
formatting are not included. Values reuse the existing Rust writer contract.

## API and output

`attribute(name, value)` returns `Result<Attribute<'_, V>, InvalidAttributeName>`.
The name is borrowed; the value may be owned or borrowed. It does not allocate
or buffer while rendering. The public `AttributeValue` trait supports custom
values explicitly rather than silently formatting arbitrary `Display` types.
Strings, characters, `HtmlString`, all existing integer/float types, bool, `Option<V>` and
references (including `&dyn AttributeValue`) are supported.

| Input | Output |
| --- | --- |
| `attribute("checked", true)` | ` checked="checked"` |
| `attribute("checked", false)` | nothing |
| `attribute("class", None::<&str>)` | nothing |
| `attribute("class", Some(""))` | ` class=""` |
| `attribute("title", "<&\"")` | ` title="&lt;&amp;&quot;"` |
| `attribute("data-char", '&')` | ` data-char="&amp;"` |
| `attribute("title", HtmlString::from("&lt;"))` | ` title="&lt;"` |

`Some(false)` omits and `Some(true)` repeats the name too. Ordinary strings
containing `&lt;` emit `&amp;lt;`: they have not been marked trusted. Attribute
names preserve spelling/case; true is conditional for any accepted name,
not just names classified as HTML boolean attributes. A literal string
`"false"` is retained, unlike the bool `false`.
Characters use the existing string encoder, including its Unicode/control
policy, via a four-byte UTF-8 stack buffer; `None::<char>` omits the attribute
and `Some(char)` delegates. There is no implicit `Display`/collection conversion.
See [bounded C# character evidence](../../docs/parity.md#character-values-and-primitive-outcomes-79).

Names match `[A-Za-z_:][A-Za-z0-9_:.-]*`. Invalid names return a structured
error with the first byte offset (or `None` for empty names). No output is
written at construction; invalid names cannot construct an adapter, even with
an omitted value. Non-ASCII values work unchanged; non-ASCII names are rejected.
This narrower name policy is an intentional Rust safety boundary, not a claim
of exact upstream name validation.

`HtmlString` keeps upstream raw dispatch. Only use it for content already safe
inside a double-quoted attribute: a trusted HTML body fragment can still contain
quotes that break attribute boundaries. Custom `AttributeValue` implementations
have the same explicit responsibility. Encoding is not URL/script/CSS
sanitization; user-controlled event handlers or dangerous URL schemes remain
the caller's responsibility.

The adapter emits one leading space and fixed double quotes; do not add your own
attribute delimiters around it. It does not flush or cache a result. `write_all`
handles short/interrupted writes. Destination/custom-value errors return
unchanged, with no suffix or subsequent write after failure; partial output
may already have been emitted.

## Runnable model-driven example

From this branch:

```sh
cargo run --quiet -p razor-slices --example attributes
```

Full exact stdout, including the final newline:

```html
<input type="checkbox" checked="checked" class="" aria-label="Wash &lt;dishes&gt; &amp; &quot;cups&quot;">
<input type="checkbox" aria-label="Call &#39;Grandma&#39; 世界">
```

The example asserts equality to `tests/fixtures/attributes.html` and renders the
same borrowed models twice. `tests/attributes.rs` exercises its rendering
function against that same complete fixture in CI. It uses ordinary byte
rendering, not a macro dependency or a change to the shared Todos sample.

## Consumer integration and concurrency

The direct `WriteHtmlValue` adapter works on main's runtime; text consumers
cannot simply interpolate it through `Display`. The integrated #31/#33
byte-to-text bridge consumes it without parser changes:

```rust,ignore
let page = html! {
    "<input"
    {attribute("checked", model.completed).expect("constant valid name")}
    {attribute("title", model.label).expect("constant valid name")}
    ">"
};
```

Dynamic names must handle `InvalidAttributeName` before rendering (or map it to
an `InvalidInput` I/O error in an existing Rust expression). Do not ignore errors
or use `expect` with untrusted names.

Generated `async_template!` interpolation accepts the same built-in adapters
via `AsyncWriteHtmlValue`. Its `AsyncAttributeValue: AttributeValue + Sync`
contract shares omission decisions but explicitly awaits typed async value
writes; it never buffers arbitrary synchronous `AttributeValue` output.
References/`Option` delegate, booleans repeat/omit the name, empty strings
remain, and trusted `HtmlString` retains the quoted-context responsibility above.
Custom attribute values must opt into both contracts. The fixed encoder and
numeric scratch buffers are independent of value length; name/trusted writes
borrow input directly. No automatic flush or URL policy is added.

```sh
cargo run -p razor-slices --example async_values --features tokio
cargo test -p razor-slices --test async_templates --all-features
```

See [the generated value guide](../../docs/async-templates.md) for exact stdout,
the observed prefix-before-await handshake and real HTTP acceptance.

`tests/fixtures/inline_attributes.rs` is an opt-in integration probe for an
isolated combined checkout with pinned #33
`abeaad6d3a8648e8f7bfe2d166d2d3716da6a1fc` (including #31
`e44e961c4b39552d679726a5391f278b8b1be472`), not a default test on this branch.
After combining those changes locally:

```sh
cp crates/razor-slices/tests/fixtures/inline_attributes.rs \
   crates/razor-slices/tests/inline_attributes.rs
cargo test -p razor-slices --test inline_attributes
cargo +1.75 test -p razor-slices --test inline_attributes
```

The probe covers exact model/partial fixture output, repeated text and byte
rendering, trusted dispatch, nested original I/O errors and text sink failures.
All four probe tests passed on stable and Rust 1.75 in that isolated combined
checkout; both adapter commits applied without conflicts to the pinned #33 head.
It does not modify or depend on the concurrent control-flow parser branch or
Axum workspace changes. Combining branches may require additive reconciliation
in `lib.rs`; no existing branches are mutated. Text rendering retains that
consumer's `fmt::Error`/`RenderError` boundary; byte rendering retains original
`io::Error` details.

## Verification and remaining gaps

Regression tests cover the upstream bool/null/empty matrix, escaped/unescaped
trust distinction, Unicode, all existing numeric types, borrowed names/models,
trait-object values, repeated renders, invalid-name injection/control cases,
short/interrupted/zero writes, omitted values on a failing sink, original custom
value errors and destination failure at every byte (including UTF-8/entity
boundaries). Compile-fail doctests reject unsupported values and escaping name
or model lifetimes.

Stable and Rust 1.75 default/all-feature build/test and exact-output example
checks, stable formatting/clippy and deny-warning docs passed locally. Exact-head
Actions must pass before Ready. The existing CI matrix builds the example and runs the fixture regression;
The #62/#63 follow-up adds its mixed-value example to the existing stable/MSRV
fixture checks without dropping original regression coverage.

This is not a full Razor port: multi-value/prefixed parts, source offsets,
literal-part flags, compiler data-* exceptions, object reflection/ToString
fallback, .NET numeric culture and default HtmlEncoder byte parity remain
outside the contract. Syntax/declarations/control flow remain #3; layout/partial
context and async lifecycle remain their own roadmap work. #43 closes only this
bounded adapter delivery.
