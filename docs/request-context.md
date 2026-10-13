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
