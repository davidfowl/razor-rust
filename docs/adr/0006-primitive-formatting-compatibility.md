# 0006. Primitive formatting compatibility with C# RazorSlices

## Status

Proposed (recommendation pending input from @davidfowl / @DamianEdwards; originating issue [#99](https://github.com/davidfowl/razor-rust/issues/99))

## Context

The pinned C# reference (`552937e5`, 0.12.1) differs from Rust for booleans,
infinities, exponent notation, single quotes and non-ASCII scalars. Eighteen
cases are recorded in `tests/parity/fixtures/primitive-outcomes.txt`. Direct
`Write(bool?)` on `PipeWriter` emits nothing, which is a reference defect.

## Decision

Recommended: retain current Rust output for all differing cases, document each
as an explicit divergence, and pin it with the `primitive_outcomes` test.
Alternatives: (a) match C# (`True`/`False`, `Infinity`, `1E-30`) for formatting,
at the cost of changing existing default output; (b) opt-in culture/format
policy; (c) match `&#x27;` and numeric references for non-ASCII via the encoder
policy tracked in #98. The direct-`bool` `PipeWriter` discrepancy is never copied.

## Consequences

No default output changes now. If an alternative is accepted, the pinned
"retained" assertions flip to equality against the C# fixture.
