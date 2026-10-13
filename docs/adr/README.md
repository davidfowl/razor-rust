# Architecture Decision Records

This directory tracks the significant, hard-to-reverse design decisions made
while porting [RazorSlices](https://github.com/DamianEdwards/RazorSlices) to
Rust (tracking epic: [#1](https://github.com/davidfowl/razor-rust/issues/1)).
It exists so the tradeoffs behind a decision — and who was asked to weigh in —
stay visible without having to dig through issue history.

## When to add an ADR

Add an ADR for a genuine architecture/design tradeoff: something that's
costly to unwind once code is written against it (e.g. a new dependency, a
public API shape, a codegen mechanism). Routine implementation detail,
bug fixes, and straightforward feature work do not need one.

**Going forward:** whenever a new design-decision comment is posted on an
issue (options + a recommendation, tagging @davidfowl / @DamianEdwards for
input), it should get a corresponding ADR file in this directory — either in
the same PR that adds the comment, or as a prompt follow-up immediately
after.

## Template

Each ADR is a single Markdown file named `NNNN-kebab-case-slug.md`, numbered
sequentially (`0001-`, `0002-`, …). Use this template:

```markdown
# NNNN. Title

## Status

Proposed | Accepted | Superseded by [NNNN](./NNNN-slug.md)

## Context

What problem/tradeoff prompted this decision? Link the originating GitHub
issue/comment(s).

## Decision

What was decided (or recommended, if still Proposed), and the alternatives
considered.

## Consequences

What this makes easier, harder, or forecloses going forward.
```

## Index

| # | Title | Status |
|---|-------|--------|
| [0001](./0001-template-syntax-and-codegen-mechanism.md) | Template syntax & codegen mechanism | Accepted (inline proc-macro mechanism; contracts and implementation pending) |
| [0002](./0002-async-runtime-dependency-for-streaming-rendering.md) | Async runtime dependency for streaming rendering | Accepted (implemented) |
| [0003](./0003-byte-oriented-zero-allocation-html-encoding.md) | Byte-oriented, zero-allocation HTML encoding | Accepted (implemented) |
| [0004](./0004-ci-hardening-and-gated-crates-io-release-workflow.md) | CI hardening & gated crates.io release workflow | Accepted (implemented) |
| [0005](./0005-simd-html-encoding.md) | SIMD-vectorized HTML encoder byte scanning | Accepted (implemented) |
