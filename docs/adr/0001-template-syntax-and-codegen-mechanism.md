# 0001. Template syntax & codegen mechanism

## Status

Accepted for the template authoring model and codegen mechanism:
inline proc-macro templates (option A). [davidfowl's reply on #3][acceptance]
accepts A from the [original options proposal][comment]. Codegen is not
implemented; the initial public contracts remain undecided.

## Context

Upstream RazorSlices uses a Roslyn incremental source generator that parses
`.cshtml` files and emits strongly-typed proxy types at compile time. Rust
has no direct equivalent, so [#3](https://github.com/davidfowl/razor-rust/issues/3)
("Epic: Template syntax & codegen") needs to settle the template authoring
model and codegen mechanism before implementation starts — a hard-to-reverse
choice once templates are authored against it.

Three options were posted on the issue, [tagging @davidfowl / @DamianEdwards][comment]:

The examples and tooling tradeoffs below summarize that historical proposal,
not an accepted macro name, grammar, or guarantee of IDE support.

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

**Accepted: A** (proc-macro, inline templates). In response to the original
A/B/C proposal, davidfowl wrote ["A looks good."][acceptance] This selects
inline authoring and proc-macro expansion, rather than external template
files (B) or `build.rs` codegen (C).

**Historical recommendation: B** (proc-macro + external template file).
The original proposal recommended B for its closer architectural match to
upstream, ecosystem precedent (Askama/`rinja`), and conceptual fit with
`_ViewImports`/`@inherits`-style model binding. Its suggested Razor-like
subset (interpolation, `@if`/`@for`, code blocks) belonged to that proposal;
neither B nor that syntax was accepted.

The acceptance does not settle the initial macro return type (`Slice`
versus `HtmlString`), declaration form, model contract, grammar, or async
behavior. Those remain open in #3; this record does not select any of them.

## Consequences

The A/B/C mechanism choice is no longer a blocker. Inline templates avoid
requiring a separate template file or `build.rs` for the selected authoring
model, but need a parser and proc-macro implementation with an agreed public
contract. IDE support must be assessed against that actual implementation.

Layouts (#4), partials (#5), and framework integration (#6) still depend on
#3's remaining contract decisions and codegen work. Acceptance of this
mechanism does not complete those epics or promise incremental async
rendering, which remains tracked separately in #11.

[comment]: https://github.com/davidfowl/razor-rust/issues/3#issuecomment-6130303507
[acceptance]: https://github.com/davidfowl/razor-rust/issues/3#issuecomment-6132090066
