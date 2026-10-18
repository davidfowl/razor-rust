# Equivalent-output rendering comparison

This bounded slice of [#8](https://github.com/davidfowl/razor-rust/issues/8)
compares **razor-rust, Askama, Maud, Tera and C# RazorSlices 0.12.1** on
one typed repeated-list HTML workload. It is not full #8 completion, a
full Razor parity claim, an HTTP/streaming benchmark, or a performance promise.
The existing encoder example is a separate microbenchmark.

## Reproduce

Requires Python 3.9+, Cargo/Rust 1.75+, Git, and the **.NET SDK 10.0.402**
(including ASP.NET Core runtime). The benchmark is a separate Rust workspace;
its checked-in lockfile does not change the runtime's Rust 1.75 baseline.
The lock also constrains transitive `ignore`, `globset`, and `thiserror` releases
because newer resolutions were not Cargo 1.75-compatible; use `--locked`.
All paths below are relative to the repository root. Choose an unused upstream
destination; never reset or modify an existing reference checkout.

```sh
git clone https://github.com/DamianEdwards/RazorSlices.git /tmp/render-comparison-upstream
git -C /tmp/render-comparison-upstream checkout --detach 552937e5f2f21ebcdc73939be014f3d217ffbb22
python3 benchmarks/render-comparison/run.py \
  --upstream /tmp/render-comparison-upstream --mode smoke --verify-rejection
python3 benchmarks/render-comparison/run.py \
  --upstream /tmp/render-comparison-upstream --mode measure --verify-rejection \
  --output benchmarks/render-comparison/results/measure.json
```

Both commands build optimized Release binaries, require a completely clean
pinned upstream checkout, and exit nonzero on any build, rendering, or equality
failure. No generated output is normalized. All five engines must pass a
two-render exact UTF-8 byte comparison with the same checked-in `expected.html`
**before either process can time any rendering**. Native runners repeat their
own gate before measurement. `--verify-rejection` appends deliberately wrong
bytes to a temporary expected file and requires every engine to report an
output mismatch, fail, and emit no measurement samples. Smoke mode repeats
the actual rendering/allocation protocol with tiny counts, not fake data.

Results are JSON (raw samples, medians, gate and negative-control evidence,
fixture/expected/lock/harness/generated-C# hashes, complete Cargo package
versions, compiler/runtime versions, hardware/OS/cgroup information).
`results/`, Rust build output, and C# build/generated output are ignored.

## Workload and boundaries

`fixture.json` defines four typed name/detail rows repeated four times.
Title and row fields contain ordinary ASCII plus `&`, `<`, and `>`.
The common encoder subset is ASCII letters, digits, spaces, `&`, `<`, and `>`.
Quotes, slash, other punctuation and non-ASCII are deliberately excluded:
encoder policies and entity spellings differ between engines. Every model
string is validated against the subset; this is
not evidence of broader encoder parity. The checked-in expected HTML includes
one trailing LF. There are no trusted/raw interpolation bypasses.

The Rust port uses generated `html!`; Askama uses its compiled template
(`askama-list.html` explicitly emits LF because Askama strips the source's final LF),
Maud its `html!` macro, and Tera its parsed `list.html` template with normal
autoescaping. C# uses SDK-generated `List.cshtml` inheriting the **unaltered
runtime sources** from the exact upstream commit. It is not handwritten
interpolation or an emulation of RazorSlices. Like existing differential
projects, it does not run upstream's convenience source generator: discovery
and a compiled typed constructor delegate are set up outside measurement.
Generated Razor code is retained in `csharp/obj/generated`.

SDK 10.0.402 and its C#/Razor compiler are deliberately pinned for this harness.
This qualifies the upstream 0.12.1 runtime with that SDK, not the generator
behavior of every SDK or the published NuGet package's build targets.
Both the compiler version and Razor compiler binary hash are recorded.
This is a TextWriter materialization comparison, not upstream's native
PipeWriter/UTF-8 fast path.

All model parsing/expansion, Tera parsing/context serialization, compiled
template setup, constructor delegate compilation, file I/O, verification,
JSON serialization and reporting are outside the measured render loops.
Each render returns a fresh complete owned string using normal framework APIs:
Rust `Slice::render_to_string`, Askama `render`, Maud `into_string`,
Tera `render`, and C# `RenderAsync(StringWriter)` plus `ToString`.
The C# entry constructs a fresh generated slice and StringWriter per render,
including their allocation and slice disposal; Rust's reusable template values
are prepared once. Maud creates its markup buffer inside each invocation.
There is no shared sink-capacity tuning or pooling added by this harness.
UTF-8 Rust Strings versus UTF-16 StringWriter/StringBuilder plus the final
.NET string, framework growth policies, Tera's dynamic context, slice
construction and managed collection are **inherent non-equivalences**, not
adjusted away. UTF-8 conversion in the C# correctness gate is not timed.

## Protocol and metrics

Single-threaded, fixed engine order (razor-rust, Askama, Maud, Tera, then C#),
one process for the four Rust engines and a separate .NET process. No concurrent
benchmark jobs, process isolation per engine, affinity pinning, forced GC,
adaptive stopping or statistical significance claims are supplied.

| Mode | Warmup renders/engine | Samples/engine | Timed renders/sample | Allocation-pass renders/sample |
| --- | ---: | ---: | ---: | ---: |
| smoke | 2 | 1 | 2 | 2 |
| measure | 1,000 | 5 | 5,000 | 5,000 |

Each warmup is followed by alternating timing and allocation passes. Timing
uses monotonic `Instant`/`Stopwatch`, including render, owned-output
materialization and Rust drop/C# writer disposal, excluding the reporting
and allocation-counter readout. Every returned output reaches
`black_box`/`GC.KeepAlive`. Throughput is completed renders divided by elapsed
seconds; output bytes per render are the shared UTF-8 fixture length, **not**
the C# in-memory string size. Raw elapsed nanoseconds and per-sample
throughput are retained; median throughput summarizes only this run.

Rust's benchmark-only global allocator forwards to `System` and counts
successful `alloc`, `alloc_zeroed`, and `realloc` requests only in the separate
allocation pass. Each successful realloc counts as one event and its entire
new requested size; deallocations are not subtracted. Bytes are **cumulative
requested bytes**, not retained/peak heap, physical RSS, or allocator overhead.
The allocation instrumentation is disabled for timing, but its atomic flag
check on allocation remains overhead. No thread or other work is spawned.
Before any rendering, a known-size calibration exercises allocation (64 bytes),
reallocation (128 bytes), zeroed allocation (32 bytes), and deallocation, and
requires exactly three events / 224 requested bytes.

C# uses `GC.GetAllocatedBytesForCurrentThread` deltas in a separate synchronous
render pass; a render that does not complete synchronously is an explicit
failure. Bytes are **managed allocation bytes**, including object headers
and alignment, not native memory, retained heap or .NET allocation events.
Allocation-event fields are `null` for C# because this counter cannot measure
them. Counters are divided by allocation-pass renders, not timed renders from
a different sample. They are genuine but **not identical cross-language
accounting units**. No zero-allocation or superiority claim follows.

.NET tiered compilation, JIT, GC and shared-machine noise can influence these
short fixed runs despite warmup. Repeat fresh invocations on controlled
hardware for decisions; cloud observations are not release guarantees.
There are no arbitrary performance pass/fail thresholds.

## Recorded cloud observation and qualification

[Release evidence](../benchmarks/render-comparison/evidence/measure.json) and
[smoke 1](../benchmarks/render-comparison/evidence/smoke-1.json) /
[smoke 2](../benchmarks/render-comparison/evidence/smoke-2.json) retain the raw
results from clean source commit
`8935b90795297ac3e20a0cf470c4aadc25d85a4f`; the subsequent evidence commit
only adds these artifacts and this documentation. All three invocations passed
five exact **960-byte** gates and deliberate-mismatch rejection for all engines.
Fixture SHA-256:
`674c5f07c9904dbd9136f34d69b1d16c4403535765daf91a79eb6536b209ee77`.
Expected HTML SHA-256:
`d1d94df8149f03d8bdb7c968700a3cc2bba1e8b48953248bab53990feb1aac92`.

The release observation used Rust 1.99.1 / LLVM 23.1.1, .NET SDK 10.0.402,
C# compiler `5.9.0-1.26505.111`, .NET/ASP.NET Core runtime 10.0.13,
Linux x86-64 / glibc 2.39, and AMD EPYC 9V74 with two visible/affinity CPUs.
Container CPU/memory quota files were unavailable; this does not prove unlimited
resources. Full version/environment details and all samples are in the JSON.

| Engine | Median renders/s | Allocated bytes/render | Rust allocation requests/render |
| --- | ---: | ---: | ---: |
| razor-rust 0.1.0 (this source) | 377,252 | 2,040 | 8 |
| Askama 0.12.1 | 790,181 | 2,356 | 5 |
| Maud 0.26.0 | 1,925,212 | 3,038 | 5 |
| Tera 1.19.1 | 153,890 | 5,031 | 135 |
| C# RazorSlices 0.12.1 pinned runtime | 123,853 | 4,752 (managed) | not measured |

These are observations of **different materialization paths and allocation
accounting**, not a ranking of the engines' general capabilities. C# throughput
ranged from about 78,624 to 148,566 renders/s within this short run. Smoke timings
are intentionally too short to interpret;
neither outliers nor slower samples were discarded.

Qualification commands, run in addition to the three recorded invocations:

```sh
cargo +1.75.0 build --manifest-path benchmarks/render-comparison/Cargo.toml --release --locked
# Immediately after that build, run its native binary to qualify 1.75-generated output:
benchmarks/render-comparison/target/release/render-comparison \
  verify benchmarks/render-comparison/fixture.json benchmarks/render-comparison/expected.html 2 1 2
cargo fmt --manifest-path benchmarks/render-comparison/Cargo.toml -- --check
cargo clippy --manifest-path benchmarks/render-comparison/Cargo.toml --release --all-targets --locked -- -D warnings
python3 -m py_compile benchmarks/render-comparison/run.py
cargo test -p razor-slices --lib
cargo test -p razor-slices-macros --lib
```

Rust 1.75 release compilation and exact native Rust output passed; the existing
runtime's 37 and macro crate's 14 unit tests passed. C# Release builds passed
with warnings treated as errors. The allocator calibration passed before every
native Rust invocation. Evidence arithmetic, source cleanliness and identical
harness hashes across the three runs were checked separately.

Two real qualification failures were resolved, not hidden: Cargo 1.75 rejected
the initial transitive `ignore` 0.4.30 resolution's edition-2024 manifest, so
the standalone lock was constrained to compatible versions; the first equality
run rejected Askama's 959-byte output versus the 960-byte expected file, so
its template now explicitly renders LF. No normalization or handwritten
framework substitute was introduced, and no timing was allowed after that
failed gate.

Known gaps remain broader workloads/escaping, streaming/HTTP output, native
PipeWriter comparison, controlled-hardware statistical benchmarking, full
upstream source-generator/package-target parity, and the rest of #8's acceptance.
