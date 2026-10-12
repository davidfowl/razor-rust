# Razor Slices for Rust

A lightweight, high-performance template library for Rust, inspired by
[RazorSlices](https://github.com/DamianEdwards/RazorSlices) for ASP.NET Core:
compile-time checked templates, low allocations, and (soon) async/streaming
rendering — without pulling in a full web/MVC framework.

> **Status: early scaffolding.** The core rendering runtime exists; template
> codegen, layouts, partials, and framework integration are not yet
> implemented. See [#1](https://github.com/davidfowl/razor-rust/issues/1) for
> the full tracking epic and current progress.

## What works today

- A `Slice` trait (`crates/razor-slices`) for rendering a template to a
  `String` or `Vec<u8>`, with HTML-encoding of interpolated values —
  implemented by hand for now (see the example below).
- CI on GitHub Actions: build, test, `clippy`, and `rustfmt` checks on every PR.

```rust
use razor_slices::{encode_html_to, Slice};
use std::fmt;

struct Hello<'a> {
    name: &'a str,
}

impl Slice for Hello<'_> {
    fn render(&self, writer: &mut dyn fmt::Write) -> fmt::Result {
        writer.write_str("<p>Hello, ")?;
        encode_html_to(self.name, writer)?;
        writer.write_str("!</p>")
    }
}

let html = Hello { name: "World" }.render_to_string().unwrap();
assert_eq!(html, "<p>Hello, World!</p>");
```

## What's next

- [#3](https://github.com/davidfowl/razor-rust/issues/3) — template syntax
  and codegen, so templates are authored as template files rather than
  hand-written `Slice` impls
- [#4](https://github.com/davidfowl/razor-rust/issues/4) — layouts & sections
- [#5](https://github.com/davidfowl/razor-rust/issues/5) — partials
- [#6](https://github.com/davidfowl/razor-rust/issues/6) — web framework
  integration (axum first)
- [#11](https://github.com/davidfowl/razor-rust/issues/11) — async/streaming
  rendering

Full breakdown and live status: see the sub-issues checked off in
[#1](https://github.com/davidfowl/razor-rust/issues/1).

## Building

```shell
cargo build --workspace
cargo test --workspace
```

## Credits

Architecture and API surface are directly inspired by
[DamianEdwards/RazorSlices](https://github.com/DamianEdwards/RazorSlices).
