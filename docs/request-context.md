# Request-scoped incremental rendering

Bounded follow-up to #52/#11, based on Ready/unmerged #63 at
`43056c1cc10f96c92cdfd0c04aeec42b745024c3`. This is not landed runtime parity.

## Approach

- Keep existing rendering entry points and typed template contracts unchanged.
- Add runtime-agnostic cooperative cancellation with an observable original
  `Interrupted` error and cancellation source, never successful EOF.
- Carry immutable typed request data through existing borrowed models, while
  the same token follows the incremental writer through children and sections.
- Wire cancellation into the body-owned Axum renderer without a producer task,
  and distinguish portable cancellation from HTTP transport/body-drop behavior.
- Verify held awaits, pending writes, isolation, repeated renders and exact output.

Async declared layout dispatch remains the independent #47/#65 delivery.
Automatic service injection, a portable response-start API and broader lifetime
parity remain outside this milestone.

## Portable contract

`RenderContext<T>` owns an immutable typed request value and a cloneable
`CancellationToken`. `T` may itself borrow request state: no `Any`, downcast,
`'static` requirement, mutable global context or template-local request cache.
Bind `&RenderContext<T>` through ordinary template models; explicitly pass that
same binding when constructing children/sections. The request data is **not**
automatically injected into unrelated models.

`render_incremental_with_context(&page, &mut writer, &context)` executes on
the existing `AsyncSlice` contract. `AsyncHtmlWriter::cancellation_token()`
automatically exposes the same token to handwritten/generated children, lazy
sections and explicit layout composition because they share the output. It
also races every destination write/flush against cancellation. No signature
changes to existing templates, `AsyncSliceWriter`, value adapters or generated
syntax are required. Legacy `render_incremental` remains unscoped and unchanged.

Cancellation races the entire scoped execution, so a pending application await
is dropped even if it does not explicitly observe the token. Application code
can additionally use `token.cancelled().await` or `token.run(fallible_work).await`.
Every poll checks cancellation first; if work is ready in that poll, its original
result is retained. An already-cancelled scope never polls the template.
This is cooperative: code that blocks inside `poll` cannot be interrupted.

Cancellation returns `AsyncRenderError::Write(io::Error)` with kind
`Interrupted` and a downcastable `RenderCancelled` source, using the existing
error variants. It never becomes successful output. There is no suffix or
final flush after cancellation/error. Pending writes may already have accepted
bytes; there is no rollback, retry or writer shutdown. Explicitly spawned work
is not aborted and must observe a token itself.

Repeated successful renders rerun the same borrowed model with fresh execution
state. Cancellation is one-shot and persists across repeats; use a fresh token
for a new request, never reset/reuse a cancelled scope. Dropping a core token or
render future does **not** signal token cancellation: future-drop still stops
polling, and caller-owned scopes retain their explicit lifetime. Observer
wakers unregister on drop; token storage follows peak simultaneous observers,
not the historical number of waits. Tokens allocate shared state and observer
storage, not rendered output.

## Runnable exact output

```sh
cargo run --locked -p razor-slices --example request_context --features tokio
```

The sample renders a generated page with a lazy header section and child from
the same borrowed typed request. Its **one-byte** duplex reader compares the
entire prefix and observes the application await being entered before releasing
it. It renders the same page/context successfully twice, then explicitly
cancels a third render during that await. No sleep or timing threshold:

```text
Render 1 prefix: <article><h1>&lt;request&gt; &amp; 雪</h1>
Render 1 full: <article><h1>&lt;request&gt; &amp; 雪</h1><p>&lt;request&gt; &amp; 雪</p></article>
Render 2 prefix: <article><h1>&lt;request&gt; &amp; 雪</h1>
Render 2 full: <article><h1>&lt;request&gt; &amp; 雪</h1><p>&lt;request&gt; &amp; 雪</p></article>
Render 3 prefix: <article><h1>&lt;request&gt; &amp; 雪</h1>
Cancelled: Interrupted
```

[Runnable source](../crates/razor-slices/examples/request_context.rs),
[typed model](../crates/razor-slices/examples/support/request_context.rs) and
[exact stdout fixture](../crates/razor-slices/tests/fixtures/request_context.txt).

## Axum contract

Each `StreamingSliceResponse` creates a dedicated token. Clone it through
`response.cancellation_token()` before `into_response`, or supply the model's
token with `with_cancellation_token(context.cancellation_token().clone())`.
Owned HTTP models can hold `Arc<RenderContext<T>>`; streaming bodies still
require `Send + 'static`. Borrowed core models cannot escape into HTTP bodies.

An explicit application cancellation wakes held rendering and produces the
same logged body error/source after already accepted queued frames. On local
HTTP/1 this fails the transfer, retaining the selected status instead of
returning a successful EOF or replacement 500. Dropping an **unfinished** body
signals its dedicated token and drops its owned render future. This includes
an unpolled body (for example HEAD). Successful render completion/body drop
does not signal cancellation. A completed renderer with undelivered queued
bytes is also no longer an unfinished render.

The adapter does not spawn a producer or acquire/inject Axum request metadata
or services. Do not reuse its token for independent requests. The token is a
portable signal, **not** a portable peer-disconnect detector: Hyper/Axum still
controls when a disconnect drops the body. Existing #55/#57 transport tests
remain the evidence for actual disconnect and empty-prefix HTTP/1 commitment;
their disconnect tests now additionally assert scope token notification.
No middleware/proxy/HTTP2 deadline or explicit `StartAsync` API is promised.

## Pinned upstream comparison

At [DamianEdwards/RazorSlices@552937e5f2f21ebcdc73939be014f3d217ffbb22](https://github.com/DamianEdwards/RazorSlices/tree/552937e5f2f21ebcdc73939be014f3d217ffbb22):

- `RazorSlice.cs` stores `HttpContext`, lazily resolves `ServiceProvider` from
  `RequestServices`, assigns the explicit token on both writer rendering paths,
  and passes it to `FlushAsync` and pipe auto-flush. `CopySliceState` copies
  HTTP context and the explicitly supplied service provider, not a new scope.
- `RazorSlice.Partials.cs` copies that state and renders children with the
  parent's token and writer, bypassing declared layout rendering.
- `RazorSlice.cs` also copies state to a newly created declared layout and
  renders it with the original token. `RazorLayoutSlice.RenderBodyAsync`
  uses the child-render path; `RenderSectionAsync` directly executes the
  immediate content's section rather than assigning a new request/token.
- `RazorSlice.IResult.cs` assigns the HTTP context, calls `Response.StartAsync`,
  and passes `RequestAborted` to rendering and the final response flush.

Rust keeps immutable context in typed models and execution state in the output,
not mutable/disposable reusable slice instances. It additionally races the
whole render against cancellation; upstream arbitrary application awaits
require application cooperation and are not automatically interrupted by
`RequestAborted`. This stronger opt-in Rust scope is explicit, not claimed
identical .NET application-await semantics. Upstream synchronous buffer writes
and native pipe flags are not replicated. Successful completed slices/layouts
do check the inclusive 16,384-byte threshold in this consolidated source.

## Known gaps / follow-ups

- [#69](https://github.com/davidfowl/razor-rust/issues/69) tracks broader
  request-scope/declared-layout acceptance. This consolidation includes both
  #68 and #65 through #73. Added regressions exercise repeated generated nested
  layouts, cancellation inside a held child section, shared threshold accounting
  and cancellation during an automatic flush. That bounded coverage does not
  claim the entire #69 acceptance scope.
- #52 retains automatic HTTP request metadata/service association and portable
  response start; generic request data is explicit in this milestone.
- #11 retains native writer completion/accounting flags, disposal/initialization
  lifecycle and broader encoder policy. The synchronous buffered bridge is
  unchanged and has no new cancellation API.
