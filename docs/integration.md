# Combined-source preview

Focused [#60](https://github.com/davidfowl/razor-rust/issues/60),
[PR #61](https://github.com/davidfowl/razor-rust/pull/61) assembles delivered
feature branches without changing main or any source PR branch. They remain
unmerged. Local merge commits assemble sources for verification; they do not
authorize repository merges, publishing or deployment.

## Exact source deliveries

| PR | Head | Scope |
| --- | --- | --- |
| #25 | `06c1d980aa3c6c2d5d4d916006bb6a94fb6284ca` | Wrapper override preservation |
| #27 | `1dc37e5bee948b09700d85556c88d3c085505025` | Contributor instructions |
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

The HTTP stack includes clean copies of #38's original commits. Combining
#49 retains the original async history as well; duplicate implementation
lines are resolved to the section-aware version, not duplicated definitions.
Export conflicts preserve synchronous layout/section/attribute APIs alongside
incremental async APIs. Wrapper integration retains all text/byte overrides
and the newer child/section dispatch paths.

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

## Remaining work

Typed generated async templates from verified Ready #58/#59 are included.
Their initial interpolation accepts strings, not all synchronous primitive,
trusted-HTML or attribute values; focused generated value parity is
[#62](https://github.com/davidfowl/razor-rust/issues/62) under #3/#2/#11.
Reserved ADR reconciliation #28/#29 is not overwritten. Current source
instructions and README describe the assembled code, not historical
scaffolding.

This follow-up source adds typed async declarations and immediate-content
layout/section context in [#64 / PR #65](https://github.com/davidfowl/razor-rust/pull/65);
see [async-layouts.md](async-layouts.md) for exact full/partial output and its
observable section handshake. Synchronous nested provider context remains
[#66](https://github.com/davidfowl/razor-rust/issues/66), with broader layout
lifecycle in #47. The #61 source heads above remain frozen. Request context/tokens, portable
explicit response-start, lifecycle/threshold-flush and broader encoder policy
remain #52/#11. Proxy/HTTP2 behavior and complete .NET parity are not inferred
from local HTTP/1 tests. Broader benchmark parity remains #8.
