# Incremental Axum responses

Focused milestone [#50](https://github.com/davidfowl/razor-rust/issues/50),
delivered in [PR #51](https://github.com/davidfowl/razor-rust/pull/51).
This branch explicitly assembles **Ready/unmerged #40**
(`5d8600f870a262aeb5e66c464c31fcb7474a9380`, including #33/#31) and
**Ready/unmerged #38** (`64b626a4869d7b7a46a1563e52f180ded929c458`).
The two existing async commits were applied on this isolated branch without
source conflicts; none of the delivered branches was changed.
Integrate #31/#33/#40 and #38 before this milestone. These APIs are not yet
on main; the cumulative main diff includes the disclosed dependencies.

## Run a real streaming page

```sh
cargo run --locked -p razor-slices-axum --example streaming_server
```

In another terminal, keep this request open:

```sh
curl --no-buffer -i http://127.0.0.1:3000/stream
```

It receives status 200, `text/html; charset=utf-8`, and this prefix while
the renderer is waiting:

```html
<p>Loading...</p>
```

Only after receiving that prefix, release the renderer from a third terminal:

```sh
curl -i -X POST http://127.0.0.1:3000/release
```

The release request returns 204. The original streaming request then receives:

```html
<h1>&lt;世界&gt; &amp; &quot;friends&quot; &#39;雪&#39;</h1>
```

The complete bytes match `tests/fixtures/streaming.html`. The real HTTP test
receives/asserts the prefix before making a separate release request and then
compares the complete decoded response to that fixture. Its timeout detects
deadlocks; it is not a sleep-based latency assertion.
The sample uses one shared `Notify` permit and is deliberately for one
controlled stream at a time, not a production synchronization pattern.

`BIND_ADDRESS` defaults to `127.0.0.1:3000`; use `127.0.0.1:0` for an
available ephemeral port. Invalid/non-Unicode values fail explicitly.
`GET /error` demonstrates a rendering failure after partial output.
Install a tracing subscriber; the example logs
`streaming slice response rendering failed` and the original error.
The client observes a failed/truncated transfer, not a generic clean 500.
The example has no authentication, TLS or graceful-shutdown policy.

## Contract and bounded output

`StreamingSliceResponse::new(slice)` accepts an owned `AsyncSlice + Send +
'static` and implements Axum's `IntoResponse`. Conversion selects status 200
and `text/html; charset=utf-8`, but does not execute the template. Models
must be owned or genuinely `'static`, unlike the buffered adapter's finished
response, which can render borrowed models before returning.
`with_status` changes the status selected before execution; failures cannot
later replace it with 500. Standard HTTP restrictions still apply. Axum's
GET router drops the unpolled body for HEAD, without executing its template.

The response body owns a single boxed render future. Each body poll advances
it alongside a bounded channel with **one queued frame**. Destination writes
split into frames of at most **1,024 bytes**; a blocked write holds at most
one additional copied frame. After that, the writer cannot progress until
the consumer polls/drains output. There is no spawned/detached producer,
full-page buffer or unbounded queue. Memory for application-owned models and
the core encoder's existing fixed scratch buffer is separate from these
adapter-owned frames.

UTF-8 code points may cross HTTP frame boundaries; the assembled byte stream
remains UTF-8. `AsyncHtmlWriter::write_literal` accepts trusted text;
`write_value` performs ordinary bounded HTML escaping. The adapter does not
re-encode finished output or sanitize trusted markup. No configurable raw
byte escape hatch is added.

Explicit `flush` makes accepted frames available; this writer has no extra
unflushed output buffer. It does not promise an actual socket flush, browser
paint or packet timing. Hyper/framework/proxy buffering and transport
backpressure are outside the core writer's visibility. The real HTTP
handshake verifies visibility on the tested local HTTP/1 path.
The existing `render_incremental` still final-flushes only after success.

## Errors and body drop

Any render/write error is logged once at ERROR under target
`razor_slices_axum`, preserving error text/kind, and emitted as a body error
after accepted queued output. The original `AsyncRenderError` and original
I/O source remain in the HTTP body's error-source chain; Axum may add wrapper
errors. There is no automatic retry, rollback, final flush after failure,
successful EOF fallback or replacement error HTML.

Even a failure before the first rendered byte is a body error: this path
does not pre-render to decide on a clean 500. A client may see no usable
response or a truncated response depending on when the transport commits
headers. Use the unchanged buffered `SliceResponse` if atomic error responses
are required. Outer status tuples/middleware can intentionally override
response headers/status, as with other Axum responses.

On the tested local Axum/Hyper HTTP/1 path, response headers also arrive
while the renderer awaits application work **before its first write**.
Two real TCP tests hold that work, receive/assert status 201, the HTML content
type and chunked transfer encoding, then release rendering. The successful
case returns exactly `&lt;世界&gt;`; the failing case instead fails the
already-started transfer, without a replacement 500 or clean empty body.
There is no initial body prefix or explicit flush driving this handshake.
This is a transport-specific regression guarantee, not an explicit
`StartAsync` API or promise about middleware, proxy or HTTP/2 buffering.

Dropping the body drops the renderer and stops polling its application awaits,
including while a write is backpressured. There is no producer task to leak
or restart. Bytes already accepted are not rolled back and independently
spawned application work is not cancelled. Peer-disconnect detection/body
drop is controlled by Hyper/Axum; this adds no standalone request cancellation
token. Panics/allocator failures are not caught.

The real HTTP/1 disconnect tests receive a prefix over TCP, then abort and
join the client connection driver to close its socket. Before stopping the
server, they observe the server-owned rendering future's drop guard. One
renderer is suspended on an unreleased application notification; another
continuously writes fixed-size text and records actual `Poll::Pending`
destination writes. Its drop guard confirms cancellation during a pending
write, not successful completion or a later application wait. Owned client
and server task guards also abort on assertion failure or timeout.
Notifications synchronize the tests without sleeps; a timeout only detects
deadlocks. This verifies framework-triggered body drop on the supported
local HTTP/1 transport, not a standalone request-aborted token, exact TCP
buffer capacity, disconnect-detection deadline, or proxy/HTTP/2 behavior.

## Verification and remaining parity

```sh
cargo test --locked -p razor-slices-axum
cargo build --locked -p razor-slices-axum --example streaming_server
```

The HTTP suite preserves all buffered response regressions and adds streaming
lazy start, selected status/content type, prefix-before-await, original-source
error/logging, early failure, empty EOF, bounded Unicode frames/slow consumers,
drop cancellation, HEAD without execution, an actual TCP HTTP/1 release
handshake, real peer disconnects during application and destination awaits,
and empty-prefix header commitment followed by success or failure.
Workspace stable/MSRV/default/all-features CI executes the suite;
existing buffered server smoke coverage remains intact.

Grounded in pinned upstream
[`RazorSlice.IResult.cs`](https://github.com/DamianEdwards/RazorSlices/blob/552937e5f2f21ebcdc73939be014f3d217ffbb22/src/RazorSlices/RazorSlice.IResult.cs)
and the streaming sample at
[`552937e5f2f21ebcdc73939be014f3d217ffbb22`](https://github.com/DamianEdwards/RazorSlices/tree/552937e5f2f21ebcdc73939be014f3d217ffbb22).
Upstream selects/starts the response before async rendering and final flush.
This adapter likewise separates selected headers from asynchronous body
execution. Its real HTTP/1 tests additionally demonstrate headers before
the first write, without claiming identical forced-start/PipeWriter semantics.
It preserves streaming order and post-start failure rather than buffered
500 replacement.

Request cancellation tokens, writer completion flags/threshold flush,
disposal and broader .NET encoder policy remain
[#11](https://github.com/davidfowl/razor-rust/issues/11).
Real disconnect regression coverage is tracked by
[#54](https://github.com/davidfowl/razor-rust/issues/54);
empty-prefix HTTP/1 response commitment is tracked by
[#56](https://github.com/davidfowl/razor-rust/issues/56).
Request context/tokens and a portable explicit response-start API remain
[#52](https://github.com/davidfowl/razor-rust/issues/52).
Generated async templates remain [#3](https://github.com/davidfowl/razor-rust/issues/3);
inherited layout/section context remains
[#47](https://github.com/davidfowl/razor-rust/issues/47)/
[#36](https://github.com/davidfowl/razor-rust/issues/36).
Actix-web, DI and broader framework/request context remain #6.
The Rust API/bounded-queue choices are owner-delegated, not claimed explicit
maintainer approval, and this is not whole-port/integrated-stack completion.
