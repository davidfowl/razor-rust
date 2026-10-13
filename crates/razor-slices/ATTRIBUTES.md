# Explicit conditional attributes

This bounded runtime adapter implements single-value, empty-prefix attributes,
not new template grammar or the entire Razor attribute state machine.

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

The branch targets main directly and does not depend on unmerged inline macro
#31 or partial #33. The adapter will implement the existing `WriteHtmlValue`
extension point, so those consumers can interpolate it without parser changes.
Only additive exports touch `lib.rs`; encoder, Slice, macros, shared samples,
workspace membership, README and ADR files are not owned by this change.

Attribute names will use a conservative ASCII grammar, rejecting malformed
names explicitly before any writing, even for an omitted value. This is a
runtime safety boundary absent from the upstream compiler-generated name path.
It does not sanitize URL schemes or prohibit event-handler attributes.

Compound/prefixed attribute parts, compiler-specific data-* treatment, general
object dispatch, async codegen and byte-identical .NET encoding/culture
formatting are not included. Values reuse the existing Rust writer contract.
