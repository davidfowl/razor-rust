# Combined-source preview

This consolidation includes all 23 open PRs targeting main at the source
snapshot below. It starts from fresh main, merges #73 (including its existing
stack), then #68, then independent documentation #29. Git ancestry preserves
the original commits and authors without replaying stacked PRs. The prior
empty direct-access test commit is not included. Source branches and PRs
remain unchanged and open.

Focused [#60](https://github.com/davidfowl/razor-rust/issues/60),
[PR #61](https://github.com/davidfowl/razor-rust/pull/61) assembles delivered
feature branches without changing main or any source PR branch. They remain
unmerged. Local merge commits assemble sources for verification; they do not
authorize repository merges, publishing or deployment.

The bounded [#70 / PR #71](https://github.com/davidfowl/razor-rust/pull/71)
follow-up combines the separately verified #63 generated async value and #65
typed nested async layout deliveries. Both start from frozen #61.
It targets main with all unmerged dependencies below; none of the source
branches are modified. Main was refreshed at
`123e62dd695a2ec0fb4400f7a27179f6c6bc543d`.

The [#67 / PR #68](https://github.com/davidfowl/razor-rust/pull/68)
request-scope follow-up starts from verified Ready/unmerged #63 at
`43056c1cc10f96c92cdfd0c04aeec42b745024c3`. It adds opt-in typed request
models/cooperative tokens and Axum scope notification without changing #63
or frozen #61. This consolidation also includes #65 async layout dispatch
through #73's existing ancestry; broader request/layout acceptance remains #69.

## Exact source deliveries

| PR | Head | Scope |
| --- | --- | --- |
| #25 | `06c1d980aa3c6c2d5d4d916006bb6a94fb6284ca` | Wrapper override preservation |
| #27 | `1dc37e5bee948b09700d85556c88d3c085505025` | Contributor instructions |
| #29 | `c7996d99abc3f7afc57f50c46baf11da6dbd6dad` | Accepted inline-codegen ADR |
| #31 | `e44e961c4b39552d679726a5391f278b8b1be472` | Inline templates |
| #33 | `abeaad6d3a8648e8f7bfe2d166d2d3716da6a1fc` | Typed model-driven partials |
| #35 | `13e7d31a5e3605d1c936d5e65895d79b302169f7` | Explicit layout/body composition |
| #38 | `64b626a4869d7b7a46a1563e52f180ded929c458` | Incremental async rendering |
| #40 | `5d8600f870a262aeb5e66c464c31fcb7474a9380` | Buffered Axum adapter |
| #42 | `96e6b73fb82aded7fef5b07c4ecc9330e1c0b74d` | Nested conditionals and loops |
| #44 | `74d9fa74ab27b6c2cb33e91f10fec31d995e64fb` | Conditional attributes |
| #46 | `7be50a51f7a963ab397d4dd3071a54e18a01d162` | Lazy named sections |
| #49 | `9a0332bfc22c8e4a999ee26fb32ce05721a491f4` | Lazy async sections |
| #51 | `1065477f7a079f056ae715ae5e3ec4d1c108d219` | Bounded streaming Axum adapter |
| #53 | `cf3a5d875c0f5823a6620cccfa4fe96c46bd5cb9` | Declared layouts and child bypass |
| #55 | `2b891f6899f58f2c3e4384d4344c782ee6c11594` | Real peer-disconnect acceptance |
| #57 | `be83e5807137e8b62a55def0321f9c317b737fd8` | Empty-prefix HTTP/1 header commitment |
| #59 | `e10fe1cf8fa451e13264129abe916ac81183878a` | Typed generated async templates |
| #61 | `7b6a8aeac067a06f64eae656a79ccdea5b035619` | Frozen 16-delivery integration base |
| #63 | `43056c1cc10f96c92cdfd0c04aeec42b745024c3` | Generated primitive/trusted/conditional values |
| #65 | `d12959b2fe574a4f80dcd599d63ea91e3d1c0fcc` | Declared/nested async layouts, immediate-content sections |
| #68 | `cb08fadae182a36a5cabda0f8ea1a4bbf04582fa` | Typed request scope and cooperative cancellation |
| #71 | `0e4c49a8e7cab4061d56964709fbd55bc6fb2788` | Combined generated values and nested-layout acceptance |
| #73 | `351409ab6453c3808152cee7d04d612894b5ee5c` | Automatic completed-slice threshold flush |

The HTTP stack includes clean copies of #38's original commits. Combining
#49 retains the original async history as well; duplicate implementation
lines are resolved to the section-aware version, not duplicated definitions.
Export conflicts preserve synchronous layout/section/attribute APIs alongside
incremental async APIs. Wrapper integration retains all text/byte overrides
and the newer child/section dispatch paths.

The consolidation retains both cancellation and unflushed-byte state on the
shared writer. Only successful cancellable writes count toward the threshold;
requested flushes reset accounting before awaiting the cancellable destination.
Both sets of samples, fixtures and HTTP regressions remain in CI. ADR
reconciliation preserves the accepted option A and historical option B
recommendation without reverting the implemented preview to scaffolding.

Consolidation regressions additionally cover repeated scoped nested documents,
typed cancellation in their held child section, unchanged shared threshold
accounting and cancellation during an automatic completion flush. They retain
exact output prefixes and stop suffix/final-flush work on cancellation.

## Runnable cross-feature acceptance

```sh
cargo run --locked -p razor-slices --example integration > target/integration-output.html
diff -u crates/razor-slices/tests/fixtures/integration.html target/integration-output.html
cargo test --locked -p razor-slices --test integration
cargo test --locked -p razor-slices-axum --test http
```

The integration example borrows a typed model containing existing todo sample
data. A declared layout surrounds a loop of conditional rows/attributes and
inserts a lazy optional footer section. Strings are encoded exactly once,
including Unicode and quote-containing attribute values. The same slice then
renders as a bare partial, without its declared document layout or footer
insertion. The exact stdout fixture contains both forms.

Four cross-feature tests additionally verify repeated text/byte/direct output
through nested reference/box/trait-object wrappers, section forwarding,
empty models, and original destination error kind/source while failing inside
an attribute or footer. Existing individual feature suites remain intact.
The HTTP suite exercises both atomic buffered output and real incremental
delivery, peer disconnects and post-header failures.
The streaming HTTP sample now uses a generated `async_template!` declaration
with an owned model, explicit flush and application await. Its existing real
TCP test receives the prefix before releasing work and compares the same exact
UTF-8 body fixture, validating generated-code/runtime/framework composition.

```sh
cargo run --locked -p razor-slices --example async_template --features tokio > target/async-template-output.txt
diff -u crates/razor-slices/tests/fixtures/async_template.txt target/async-template-output.txt
```

This independent typed-template executable additionally observes prefixes on
both renders using its small duplex/Notify handshake, proving reusable fresh
application awaits rather than cached output or consumed futures.

Stable and MSRV 1.75 CI build/test the combined workspace in default and
all-features modes. Stable also runs warning-free Clippy/docs and exact sample
comparisons; MSRV compares the integrated sample's actual stdout as well.

## Combined async acceptance

```sh
cargo run --locked -p razor-slices --example combined_async --features tokio > target/combined-async-output.txt
diff -u crates/razor-slices/tests/fixtures/combined_async.txt target/combined-async-output.txt
cargo test --locked -p razor-slices --test combined_async --all-features
cargo test --locked -p razor-slices-axum --test http
```

The shared generated declaration accepts a borrowed `&Model` or an owned
`Model`. Both reusable documents render twice through a one-byte duplex;
the exact mixed-value header prefix arrives before held application work is
released. The generated body includes numbers, escaped Unicode, omitted false/
None attributes, retained empty attributes and explicit trusted markup. An
explicit badge layout stays composed in bare partials; the declared document
and its sections do not.

The outer layout only queries its immediate inner layout. Its `"header"` is
absent, so it does not recursively discover the page header. The inner layout
inserts that header once. After body execution the outer `"footer-links"`
dispatch invokes the inner layout's own section method with its original
child, which explicitly inserts the child's generated `"links"` section.
No cached render state, global recursive lookup, mandatory executor or
whole-page production buffer is introduced.

The complete fixture prints borrowed/owned prefixes, documents and partials.
Regression tests hold, resume, fail and drop every observed destination write
and flush boundary, checking exact byte prefixes, original error sources and
no advance under backpressure. A held flush prevents section entry even when
application work is released. Dropping an entered section stops its completion,
suffix and final flush. Reference/box wrappers preserve full/bare dispatch.
The actual HTTP/1 test repeats documents with separate POST release requests,
checks exact bodies and partials, and closes the client TCP connection during
the held section to verify renderer drop. Existing pending-write disconnect
and response-commitment regressions remain in the same suite.

Run the same shared declaration as a local server:

```sh
cargo run --locked -p razor-slices-axum --example combined_server
curl --no-buffer -i http://127.0.0.1:3000/combined
# In another terminal, after observing the mixed-value header prefix:
curl -i -X POST http://127.0.0.1:3000/release
curl -i http://127.0.0.1:3000/partial
```

Repeat the document/release pair for another fresh await. Each release permits
one held header, not all concurrent requests. `BIND_ADDRESS` can select a
different local address. This is local executable acceptance, not a hosted
preview. Stable/MSRV CI runs the actual stdout fixture in addition to runtime
and real HTTP tests. The original async-value and async-layout fixtures remain
unchanged and executable.

## Remaining work

Bounded [#72 / PR #73](https://github.com/davidfowl/razor-rust/pull/73)
adds automatic 16,384-byte **completed-slice** flush to frozen #71 at
`0e4c49a8e7cab4061d56964709fbd55bc6fb2788`. This consolidation additionally
combines request scope #68; broader #69 acceptance is not claimed complete.
The existing preview and its exact fixtures remain intact.
See [automatic completion acceptance](async-rendering.md#automatic-completion-flush)
for executable buffered-sink and real HTTP evidence and native-pipe limits.

Typed generated async templates from verified Ready #58/#59 are included.
Focused [#62](https://github.com/davidfowl/razor-rust/issues/62) under #3/#2/#11
adds bounded primitive, trusted-HTML and conditional attribute interpolation
in this follow-up source. The generated mixed-value executable and real HTTP/1
test observe an exact prefix before releasing later application work.
Custom synchronous values still require explicit async implementations.
ADR reconciliation #28/#29 is included with its historical evidence. Current source
instructions and README describe the assembled code, not historical
scaffolding.

This follow-up source adds typed async declarations and immediate-content
layout/section context in [#64 / PR #65](https://github.com/davidfowl/razor-rust/pull/65);
see [async-layouts.md](async-layouts.md) for exact full/partial output and its
observable section handshake. Synchronous nested provider context (#66) now has
[bounded C# differential coverage](parity.md), with broader layout
lifecycle in #47. The #61 source heads above remain frozen. Bounded typed request
models and cooperative cancellation are documented in
[request-context.md](request-context.md); automatic completed-slice threshold
flush is also included. Automatic request metadata/service injection, portable
explicit response-start, native-pipe lifecycle/accounting and broader encoder policy
remain #52/#11. Proxy/HTTP2 behavior and complete .NET parity are not inferred
from local HTTP/1 tests. Broader benchmark parity remains #8.
