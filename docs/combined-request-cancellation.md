# Combined declared-layout request cancellation acceptance

This closes one **test/evidence gap**, not a demonstrated production defect.
[#69](https://github.com/davidfowl/razor-rust/issues/69) explicitly retained token
identity at every combined-stack seam and cancellation during that stack's
pending destination write. Existing `request_context` tests prove cancellation
through explicit composition, while `combined_async` proves held-section
cancellation through generated declared layouts. Neither proves this entire
combination. The new tests pass against unchanged main runtime code; no runtime
fix, new API, dependency, parser change or transport policy is needed.

## Observable criterion and stack

`combined_request_cancellation` builds a borrowed page inside two declared async
layouts. It asserts **reference identity**, not merely cancellation effects,
between `AsyncHtmlWriter::cancellation_token()` and the root context's token at
each execution seam:

```text
outer body
  inner body
    original page header -> header child
    bare page body -> declared child rendered without its trap layout
  inner footer
    original page header -> header child (again, after body execution)
    footer child
  immediate inner provider handles missing "page-only" (no recursive fallback)
```

The typed borrowed request label is also checked at every seam. Full output is
exactly `OIHhPbi|FHhfooto`; a bare render of the same declared document is `Pb`.
Reference/box wrappers must preserve both dispatch and context. Layout/body
owners remain alive across successful, failed, cancelled and dropped execution,
then drop exactly once with their owning declaration.

## Shared C# evidence

The authoritative source remains **DamianEdwards/RazorSlices
`552937e5f2f21ebcdc73939be014f3d217ffbb22`**, version prefix **0.12.1**.
The dedicated `tests/combined-request-cancellation` harness compiles its
unchanged runtime source directly against the .NET 10 shared framework and
rejects a different HEAD or dirty runtime directory.

At that pin, `RazorSlice.cs` assigns the supplied token before descending into
declared layouts. `RazorSlice.Partials.cs` passes it to bare child rendering;
`RazorLayoutSliceOfTModel.cs` dispatches sections on the immediate content;
explicit flushes receive the original token. The harness checks .NET token
equality (cancellation-source identity) at every seam and at native flush entry.
Both raw PipeWriter and TextWriter paths must produce the same five rows before
the harness emits the shared fixture:

| Case | Exact result |
| --- | --- |
| Full document | Success, `OIHhPbi\|FHhfooto`, all eleven observed seams |
| Bare declared document | Success, `Pb`, only page-body and bare-child seams |
| Cooperative held section cancellation | Cancellation, `OIHhPbi\|FHh`, no footer child or suffix |
| Pending explicit native flush cancellation | Cancellation, `OIHhPbi\|FHhfoot`, no outer suffix |
| Partially accepted destination failure | Original error, `OIHhPbi\|FHhf`, no cleanup/final flush |

Each implementation asserts its entire output/prefix, exact provider order,
token identity and absence of trap-layout dispatch. C# additionally checks the
original exception object and cancellation token; Rust checks original I/O
kind/source identity. Observed token copies must all see cancellation from the
original source.

The shared fixture **does not compare successful final-flush counts**: current
Rust defaults explicitly flush the root; the raw C# paths do not. Each harness
asserts its own intended count separately. This acceptance is independent of
the opt-in root policy in [PR #82](https://github.com/davidfowl/razor-rust/pull/82)
and does not copy or require that change.

## Rust-specific pending-write acceptance

Pinned C# writes are synchronous (`PipeWriter.Advance` / `TextWriter.Write`).
They cannot honestly serve as a differential model of Rust's asynchronous
`write_all`. The remaining #69 pending byte-write criterion is therefore tested
explicitly as the Rust contract:

- All **13** combined-document write calls are separately held after accepting
  their first byte. Each is polled twice with no progress, then cancelled or
  dropped: **26** exact-prefix cases. The multi-byte `foot` write demonstrates
  true partial acceptance. No later write or final flush runs; dropping alone
  does not signal the caller's token. The caller-owned destination remains usable.
- A real **one-byte Tokio duplex**, with no sleeps or spawned producer, delivers
  `OIHhPbi|FHhf` before the reader cancels the request. The footer's `foot` write
  has accepted only `f`; the rest of the footer and outer suffix are never sent.
  The same destination then accepts caller-owned output after cancellation.
- A second request completes three times while the first request is held and
  cancelled. Token effects, provider context, typed borrowed labels and owners
  stay isolated; a cancelled context cannot restart execution.

These are four dedicated Rust tests with the optional Tokio feature, three
without it. They supplement, rather than replace, existing generated-layout,
request-context, completion-flush and transport regressions.

## Reproduction

From the Rust checkout, with the pinned C# checkout beside it:

```sh
cargo test --locked -p razor-slices --test combined_request_cancellation
cargo test --locked -p razor-slices --test combined_request_cancellation --all-features
dotnet run --project tests/combined-request-cancellation/CombinedRequestCancellation.csproj \
  -p:RazorSlicesSource="$(cd ../RazorSlices-root-reference && pwd)" \
  --verbosity quiet > target/csharp-combined-cancellation-debug.txt
dotnet run --project tests/combined-request-cancellation/CombinedRequestCancellation.csproj -c Release \
  -p:RazorSlicesSource="$(cd ../RazorSlices-root-reference && pwd)" \
  --verbosity quiet > target/csharp-combined-cancellation-release.txt
diff -u crates/razor-slices/tests/fixtures/combined-request-cancellation.txt target/csharp-combined-cancellation-debug.txt
diff -u crates/razor-slices/tests/fixtures/combined-request-cancellation.txt target/csharp-combined-cancellation-release.txt
```

Use the exact reference pin above, and run the harness from the Rust checkout,
not underneath the reference checkout's unrelated SDK-pinning `global.json`.

## Limits

No full #69/#52/#11 closure, new framework request/service injection, portable
response-start, native FlushResult flag representation, disposal/pooling or
general encoder parity is claimed. Rust races the whole scoped render against
cancellation; C# held application work must explicitly cooperate (the harness
uses `WaitAsync(CancellationToken)`). A pending native flush is a common
observable boundary; an asynchronous byte write is not. No shared production
or general parity-documentation files change.
