# Buffered Axum integration

`SliceResponse<S>` implements Axum 0.7's `IntoResponse` for any synchronous
`Slice`, including inline templates, handwritten slices, references and boxes.
The separate crate leaves the core runtime framework-independent.

This branch stacks on **unmerged PR #33** at
`abeaad6d3a8648e8f7bfe2d166d2d3716da6a1fc`, including its PR #31 commits.
The model-driven inline/partial API is not on main yet. Integrate those PRs
before this one; no edits to their delivered branches are required.

## Contract

```rust
use axum::{http::StatusCode, response::IntoResponse};
use razor_slices::html;
use razor_slices_axum::SliceResponse;

let name = "<世界>";
let response = SliceResponse::new(html! { "<p>Hello, " {name} "!</p>" })
    .with_status(StatusCode::CREATED)
    .into_response();
assert_eq!(response.status(), StatusCode::CREATED);
```

- Success defaults to 200 and `text/html; charset=utf-8`. `with_status`
  changes the successful status. Standard HTTP body restrictions still apply
  (for example, Axum's GET router strips the body for HEAD).
- Conversion renders exactly once through `Slice::render_to_writer`, into a
  private byte buffer, then validates UTF-8 before constructing the response.
  It does not use or rely on `render_to_bytes` convenience overrides.
  Borrowed models are supported: the returned response owns the finished body.
- String values escape via the existing template encoder. `HtmlString` stays
  verbatim; **only use it for trusted, already-safe markup**. The adapter
  neither encodes finished HTML again nor sanitizes trusted content.
- `try_into_response` returns the original `io::Error`, or `InvalidData` for
  malformed UTF-8, for application-owned policy/observability. It does not log.
- `IntoResponse` logs failures at ERROR under target `razor_slices_axum`,
  including error text and kind, and returns 500, `text/plain; charset=utf-8`,
  and exactly `Internal Server Error`. Partial HTML and internal error details
  are not sent to the client. Install a tracing subscriber to collect logs.

Wrapping a result in an outer Axum `(status, response)` tuple can override
even a failure status. Use `with_status` instead when failures must remain 500.
Application middleware can still intentionally rewrite responses.

## Run the model-driven server

From the workspace root:

```sh
cargo run --locked -p razor-slices-axum --example server
# In another terminal:
curl -i http://127.0.0.1:3000/
curl -i http://127.0.0.1:3000/error
```

The server binds loopback by default. `BIND_ADDRESS` overrides the address;
`127.0.0.1:0` requests an available ephemeral port. Invalid addresses or
non-Unicode environment values cause an explicit startup error.

The [model and page factory](examples/support/page.rs) create a typed
`Greeting`, a reusable inline greeting partial, and a page. The actual GET
response is 200 with `text/html; charset=utf-8` and this complete body:

```html
<!DOCTYPE html>
<html><body>
<p>Hello, &lt;世界&gt; &amp; &quot;friends&quot; &#39;雪&#39;!</p>
<strong>Trusted & ready</strong>
</body></html>
```

`GET /error` deliberately writes a prefix and fails. The client receives
500/plain text and only `Internal Server Error` (no trailing newline).
The installed subscriber emits:

```text
ERROR razor_slices_axum: slice response rendering failed error=deliberate example rendering failure error_kind=Other
```

This demonstration route is not a production application recommendation.
The example has no graceful-shutdown, authentication, or TLS configuration.

## Verification

```sh
cargo test --locked -p razor-slices-axum
cargo build --locked -p razor-slices-axum --example server
bash crates/razor-slices-axum/tests/server-smoke.sh
```

The smoke script runs the compiled server on an ephemeral loopback port, uses
real curl GETs, compares HTML to the checked-in UTF-8 fixture, checks both
statuses/content types, asserts the exact error body and original error log,
and terminates its own process. Dedicated `Axum HTTP smoke` CI runs it on
stable and Rust 1.75. Existing workspace CI includes the new crate in
stable/MSRV/default/all-features build/test, Clippy, fmt and deny-warning docs;
the inherited sample CI is unchanged.

Dependencies use Axum 0.7.9, Tokio 1.43.0 and Tower 0.5.2, compatible with the
workspace Rust 1.75 MSRV. The dev-only hyper-util pin avoids its newer
Rust-1.85/edition-2024 releases. The committed workspace lockfile (format 3)
also retains a compatible optional backtrace graph. The library's normal
Axum dependency has default features disabled; HTTP server/runtime features
are dev-only. No dependency on the core's Tokio adapter or PR #38 is needed.
Downstream applications own their framework/runtime dependency resolution.

## C# parity and deliberate limits

Grounded in upstream RazorSlices
[`RazorSlice.IResult.cs`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.IResult.cs):
returning a slice from a route, default status 200, and UTF-8 HTML content
type match. The older
[`RazorSliceHttpResult`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSliceHttpResult.cs)
wrapper is obsolete upstream; Rust uses a separate wrapper because a blanket
implementation of Axum's foreign trait for core slices is not permitted by
Rust's orphan rules. This is an owner-delegated implementation choice, not
a claim of explicit maintainer public-API approval.

Upstream starts the HTTP response **before** async rendering and final flush.
This adapter intentionally buffers **before** response commitment so ordinary
rendering/UTF-8 failures can return a clean 500. It is not equivalent in
streaming, time-to-first-byte, cancellation, or post-start failure behavior.
No flush-as-you-go HTTP support is delivered: that remains #6/#11; PR #38's
incremental writer contract is independently owned.

Rendering is synchronous on the calling thread, with memory proportional
to the complete page and no built-in output cap. Bound input/output sizes and
keep templates inexpensive. Panics and allocator failure are not caught.
This wrapper does not provide DI, configurable encoders/content types,
request context, cancellation tokens, automatic layout/partial propagation,
model extractors, asset hosting, or .NET's full Unicode encoder policy.
Ordinary typed Rust factories supply models; handwritten slices remain
responsible for escaping their own values.

Only the focused buffered HTTP delivery closes its subissue. Broader web
integration (#6), samples/benchmarks (#8), codegen (#3), layouts (#4), partial
context/lifecycle (#5), and async streaming (#11) remain separate work.
