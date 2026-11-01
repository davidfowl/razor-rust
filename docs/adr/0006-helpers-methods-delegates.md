# 0006. Shareable helpers, templated methods and templated delegates

## Status

Accepted (implemented as documented patterns and tests; no grammar change).
Open owner decision: whether to add optional sugar later (see Decision).

## Context

Parity issues [#101](https://github.com/davidfowl/razor-rust/issues/101)
(shareable templated helpers, upstream `RazorSliceHelpers` + `@implements`)
and [#104](https://github.com/davidfowl/razor-rust/issues/104) (templated
Razor methods and `@<div>` delegate literals) assume a missing construct in
the `Part` enum of `razor-slices-macros`. Upstream needs those constructs
because Razor templates compile to a class with no other abstraction for
reusable markup. Here, templates are already ordinary Rust values.

## Decision

Add **no new grammar**. Every upstream capability maps onto existing
language features plus `partial` (sync) or `@render` (async):

| Upstream | Rust equivalent |
| --- | --- |
| Helper shared across slices/projects (`RazorSliceHelpers`) | `pub fn helper(..) -> impl Slice + '_ { html! {..} }` in any module/crate; or an `async_template!` declaration rendered with `@render` |
| `@implements IFoo` mixing helpers into a slice | A trait with default methods returning `impl Slice` (return-position `impl Trait` in traits, stable at the 1.75 MSRV) |
| Templated Razor method in `@functions` | An inherent method returning `impl Slice + '_` |
| Templated delegate `Func<object?, HelperResult>` | A closure or `impl Fn(&T) -> S where S: Slice`; `Box<dyn Fn(T) -> Box<dyn Slice>>` for heterogeneous results |
| Async templated delegate | Out of scope upstream; use an `async_template!` declaration |

Embedding stays explicit (`{partial(helper(x))}`), preserving the existing
rule that child markup is never implicit trusted HTML and never re-encoded.
Executable evidence is in `crates/razor-slices/tests/helpers_and_delegates.rs`;
misuse is pinned by compile-fail doctests in
`crates/razor-slices/tests/helpers-negative`.

Alternatives considered:

- **`@helper name(args) { .. }` / `@<b>..</b>` literals in `html!`.** Rejected:
  duplicates `fn`/closures, would need a second parser for argument lists and
  types, hides lifetimes the compiler already checks, and gives worse
  diagnostics. The template body is deliberately a thin layer over Rust.
- **Sugar for `{partial(x)}` (e.g. `@render { x };` in sync templates).**
  Deferred, not rejected. It is purely ergonomic and can be added later
  without breaking this decision; it is the one item for the owner to weigh.
- **A `Helpers` runtime type.** Rejected: nothing to implement beyond `Slice`.

## Consequences

- Helpers are shareable across crates with ordinary visibility and semver.
- Closure delegates have one concrete return type; branches returning different
  `html!` values need `Box<dyn Slice>`, or a single `html!` with `if`/`match`
  (tested as compile-fail so the limitation is documented).
- No `Part`/parser change, so no new surface to maintain or fuzz.
- Mixed documents ergonomics (`{partial(..)}` verbosity) remain the main gap.
