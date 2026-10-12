# 0001. Template syntax & codegen mechanism

## Status

Proposed, awaiting maintainer decision ([comment on #3][comment], no reply
yet as of this writing).

## Context

Upstream RazorSlices uses a Roslyn incremental source generator that parses
`.cshtml` files and emits strongly-typed proxy types at compile time. Rust
has no direct equivalent, so [#3](https://github.com/davidfowl/razor-rust/issues/3)
("Epic: Template syntax & codegen") needs to settle the template authoring
model and codegen mechanism before implementation starts — a hard-to-reverse
choice once templates are authored against it.

Three options were posted on the issue, [tagging @davidfowl / @DamianEdwards][comment]:

- **A. Proc-macro, inline templates** — e.g. `html! { <p>Hello, {name}</p> }`
  (à la Maud). Full IDE/rust-analyzer support, no separate file or `build.rs`,
  but not Razor-like and loses the "markup file with design-time editing"
  authoring feel.
- **B. Proc-macro + external template file**, parsed at macro-expansion time
  — e.g. `#[derive(Slice)] #[slice(path = "templates/hello.rshtml", model = Todo)]`
  (à la Askama/`rinja`). Closest match to upstream's model (separate markup
  file → generated proxy type → compile-time model binding), proven pattern,
  incremental-compilation friendly. Needs a custom parser and loses
  rust-analyzer support inside the template file (without further tooling).
- **C. `build.rs` codegen** — scan a templates dir, emit a `.rs` file into
  `OUT_DIR`, `include!()` it. More flexible multi-file resolution (mirrors
  `_ViewImports.cshtml` hierarchy walking), but `build.rs` is generally
  considered worse Rust DX (slower incremental builds, harder-to-debug
  generated code) and diverges further from upstream's source-generator
  design than option B.

## Decision

**Recommended: B** (proc-macro + external template file) — closest
architectural match to upstream, best ecosystem precedent (Askama/`rinja`),
and keeps the `_ViewImports`/`@inherits`-style model binding conceptually
intact. The template syntax itself would start as a practical subset
(interpolation, `@if`/`@for`, code blocks) and expand incrementally.

No decision has been finalized — implementation is intentionally on hold
pending maintainer sign-off, since starting to author templates against the
wrong model would be costly to unwind. See [#1](https://github.com/davidfowl/razor-rust/issues/1)
for how this blocks the layouts (#4), partials (#5), and framework
integration (#6) sub-epics.

## Consequences

Until this is accepted, #4/#5/#6 remain blocked. Once decided, the chosen
mechanism determines the shape of the template parser, the proc-macro crate
boundary, and how IDE tooling support is addressed (or deferred).

[comment]: https://github.com/davidfowl/razor-rust/issues/3#issuecomment-6130303507
