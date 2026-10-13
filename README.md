# Razor Slices for Rust

A lightweight, high-performance template library for Rust, inspired by
[RazorSlices](https://github.com/DamianEdwards/RazorSlices) for ASP.NET Core:
compile-time checked templates, low allocations, and async/streaming
rendering — without pulling in a full web/MVC framework.

> **Status: usable integration preview, not a complete port.** This source
> assembles the delivered template, layout, section and HTTP features in
> [PR #61](https://github.com/davidfowl/razor-rust/pull/61), with verified async
> value/layout deliveries #63/#65 combined in [PR #71](https://github.com/davidfowl/razor-rust/pull/71). Its source PRs
> remain unmerged; this is not a claim that the preview is on main or released.
> See [#1](https://github.com/davidfowl/razor-rust/issues/1) for the live roadmap.

## What works today

- Reusable inline `html!` templates with compile-time checked values, nested
  conditionals/loops and HTML encoding. Typed Rust factories accept borrowed
  or owned models; `Slice` also remains hand-implementable.
- Direct synchronous text/byte output, lazy partials, explicit or declared
  layouts, optional named sections and conditional attributes. Declared
  layouts are bypassed for child/partial rendering; explicit composition is
  retained.
- References and boxes (including `&dyn Slice` and `Box<dyn Slice>`) forward
  all rendering methods, preserving custom `render_to_string` and
  `render_to_bytes` overrides and their errors through generic dispatch.
- Buffered async rendering via `render_async`/`AsyncSliceWriter` — a runtime-
  agnostic way to write a rendered `Slice` to a destination (e.g. an
  HTTP response body). It calls `render_to_bytes`, including any override,
  before writing and flushing the complete buffer. The separate `AsyncSlice`
  path writes incrementally, supports explicit flush, lazy async sections and
  typed declared/nested layouts with immediate-content section context,
  and honors destination backpressure without a full-page buffer. No async
  runtime is a mandatory dependency; enable the default-off `tokio` Cargo
  feature for a ready-made `tokio::io::AsyncWrite` adapter (`TokioWriter`).
  See [#11](https://github.com/davidfowl/razor-rust/issues/11).
- Axum adapters for atomic buffered HTML responses and bounded incremental
  streaming responses, with runnable servers and real HTTP/1 regressions
  for prefix delivery, peer-disconnect cancellation and post-start failures.
- Generated `async_template!` interpolation for bools/numbers, escaped
  strings/references, explicit trusted `HtmlString` and conditional attributes.
  Built-ins use fixed encoding/format scratch; custom values must explicitly
  implement `AsyncWriteHtmlValue`, not just `Display` or `WriteHtmlValue`.
- Byte-oriented, low-allocation HTML encoding: `encode_html_to_writer` writes
  UTF-8 directly to any `std::io::Write` sink (no intermediate `String`), and
  `WriteHtmlValue` formats numbers/bools into a stack buffer instead of
  allocating — see [#10](https://github.com/davidfowl/razor-rust/issues/10).
  HTML escape detection uses monotonic SIMD-backed scans, keeping search
  work linear even for escape-heavy inputs (see
  [ADR 0005](docs/adr/0005-simd-html-encoding.md)).
- CI on GitHub Actions: build, test, `clippy`, `rustfmt`, `cargo doc` (deny
  warnings), and MSRV (1.75) build/test checks on every PR. Build, test,
  lint, and docs cover both default features and `--all-features`, including
  the Tokio adapter doctest. A manually triggered, dry-run-by-default release
  workflow is ready for crates.io publishing once a `CARGO_REGISTRY_TOKEN`
  secret is configured.

```rust
use razor_slices::{html, Slice};

let name = "<World> & friends";
let page = html! { "<p>Hello, " {name} "!</p>" };
assert_eq!(
    page.render_to_string().unwrap(),
    "<p>Hello, &lt;World&gt; &amp; friends!</p>"
);
```

## Run the preview

```sh
# Full model-driven document, then the same slice as a bare partial:
cargo run --locked -p razor-slices --example integration

# Observe a section prefix before later application work completes:
cargo run --locked -p razor-slices --example async_sections --features tokio

# Generate a typed reusable async template; observe its prefix on both renders:
cargo run --locked -p razor-slices --example async_template --features tokio

# Mixed typed values, trusted HTML and conditional attributes before later work:
cargo run --locked -p razor-slices --example async_values --features tokio

# Nested declared async layouts and sections, followed by the bare partial:
cargo run --locked -p razor-slices --example async_layout --features tokio

# Both features together: borrowed/owned models, repeated documents and partials:
cargo run --locked -p razor-slices --example combined_async --features tokio

# Actual HTTP streaming; release the held page from another terminal:
cargo run --locked -p razor-slices-axum --example streaming_server
curl --no-buffer -i http://127.0.0.1:3000/stream
curl -i -X POST http://127.0.0.1:3000/release
```

The last command must run in a separate terminal while the streaming request
is open. Servers run locally; no hosted preview or deployment is provided.

| Preview | Source and contract |
| --- | --- |
| Model, control flow, attributes, layout, sections and bare partial | [Integrated example](crates/razor-slices/examples/integration.rs), [exact output](crates/razor-slices/tests/fixtures/integration.html), [integration guide](docs/integration.md) |
| Incremental rendering and async sections | [Async rendering](docs/async-rendering.md), [section handshake](docs/async-sections.md) |
| Generated typed async templates | [Grammar, ownership and actual prefix handshake](docs/async-templates.md) |
| Declared/nested async layouts and bare partial | [Immediate-content sections, handshake and exact output](docs/async-layouts.md) |
| Mixed values through nested async layouts, over bounded I/O and HTTP | [Combined sample and HTTP commands](docs/integration.md#combined-async-acceptance), [exact stdout](crates/razor-slices/tests/fixtures/combined_async.txt) |
| Atomic buffered HTTP response | [Axum sample](crates/razor-slices-axum/README.md) |
| Bounded streaming HTTP response | [Server, commands and transport limits](crates/razor-slices-axum/STREAMING.md) |

## What's next

- [#3](https://github.com/davidfowl/razor-rust/issues/3): broader generated
  template grammar and value-type parity. Typed async declarations, nested
  control flow, await/flush, child and section directives from
  [PR #59](https://github.com/davidfowl/razor-rust/pull/59) are included here;
  [focused value parity #62](https://github.com/davidfowl/razor-rust/issues/62) /
  [PR #63](https://github.com/davidfowl/razor-rust/pull/63) adds existing built-in
  values and conditional attributes. Custom Display/date formatting and broader
  encoder policy remain outside this bounded milestone.
- [#47](https://github.com/davidfowl/razor-rust/issues/47): broader layout
  lifecycle parity and [synchronous nested provider context #66](https://github.com/davidfowl/razor-rust/issues/66).
  Typed async declarations/context from [#64 / PR #65](https://github.com/davidfowl/razor-rust/pull/65)
  are included in this follow-up source, still unmerged.
- [#11](https://github.com/davidfowl/razor-rust/issues/11) /
  [#52](https://github.com/davidfowl/razor-rust/issues/52): remaining request
  context, portable response-start, lifecycle/flush and encoder-policy parity.
- [#8](https://github.com/davidfowl/razor-rust/issues/8): broader samples and
  comparable benchmarks. Integration success is not complete .NET parity.

Full breakdown and live status: see the sub-issues checked off in
[#1](https://github.com/davidfowl/razor-rust/issues/1). Design decisions and
their tradeoffs are tracked as Architecture Decision Records in
[`docs/adr/`](docs/adr/README.md).

## Building

```shell
cargo build --workspace
cargo test --workspace
```

## Contributing

See [AGENTS.md](AGENTS.md) for repository structure, current implementation
boundaries, development checks, and contribution conventions for human and AI
contributors.

## Credits

Architecture and API surface are directly inspired by
[DamianEdwards/RazorSlices](https://github.com/DamianEdwards/RazorSlices).
