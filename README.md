# Razor Slices for Rust

A lightweight, high-performance template library for Rust, inspired by
[RazorSlices](https://github.com/DamianEdwards/RazorSlices) for ASP.NET Core:
compile-time checked templates, low allocations, and async/streaming
rendering — without pulling in a full web/MVC framework.

> **Status: early scaffolding.** The core rendering runtime exists; template
> codegen, layouts, partials, and framework integration are not yet
> implemented. See [#1](https://github.com/davidfowl/razor-rust/issues/1) for
> the full tracking epic and current progress.

## What works today

- A `Slice` trait (`crates/razor-slices`) for rendering a template to a
  `String` or `Vec<u8>`, with HTML-encoding of interpolated values —
  implemented by hand for now (see the example below).
- Streaming rendering via `render_async`/`AsyncSliceWriter` — a runtime-
  agnostic way to write a rendered `Slice` directly to a destination (e.g. an
  HTTP response body) instead of building a full buffer first. No async
  runtime is a mandatory dependency; enable the default-off `tokio` Cargo
  feature for a ready-made `tokio::io::AsyncWrite` adapter (`TokioWriter`).
  See [#11](https://github.com/davidfowl/razor-rust/issues/11).
- Byte-oriented, low-allocation HTML encoding: `encode_html_to_writer` writes
  UTF-8 directly to any `std::io::Write` sink (no intermediate `String`), and
  `WriteHtmlValue` formats numbers/bools into a stack buffer instead of
  allocating — see [#10](https://github.com/davidfowl/razor-rust/issues/10).
  HTML escape detection uses monotonic SIMD-backed scans, keeping search
  work linear even for escape-heavy inputs (see
  [ADR 0005](docs/adr/0005-simd-html-encoding.md)).
- CI on GitHub Actions: build, test, `clippy`, `rustfmt`, `cargo doc` (deny
  warnings), and an MSRV (1.75) build/test job on every PR. A manually
  triggered, dry-run-by-default release workflow is ready for crates.io
  publishing once a `CARGO_REGISTRY_TOKEN` secret is configured.

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

Runnable version: [`crates/razor-slices/examples/hello.rs`](crates/razor-slices/examples/hello.rs)
(`cargo run -p razor-slices --example hello`).

## What's next

- [#3](https://github.com/davidfowl/razor-rust/issues/3) — template syntax
  and codegen, so templates are authored as template files rather than
  hand-written `Slice` impls
- [#4](https://github.com/davidfowl/razor-rust/issues/4) — layouts & sections
- [#5](https://github.com/davidfowl/razor-rust/issues/5) — partials
- [#6](https://github.com/davidfowl/razor-rust/issues/6) — web framework
  integration (axum first)

Full breakdown and live status: see the sub-issues checked off in
[#1](https://github.com/davidfowl/razor-rust/issues/1). Design decisions and
their tradeoffs are tracked as Architecture Decision Records in
[`docs/adr/`](docs/adr/README.md).

## Building

```shell
cargo build --workspace
cargo test --workspace
```

## Credits

Architecture and API surface are directly inspired by
[DamianEdwards/RazorSlices](https://github.com/DamianEdwards/RazorSlices).
