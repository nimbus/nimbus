# Adapter Expectations

This reference captures the shared expectations for adapter code across the
repo.

It complements:

- [ARCHITECTURE.md](../../ARCHITECTURE.md)
- [runtime-adapter-boundary.md](runtime-adapter-boundary.md)
- [server-auth-runtime-trust.md](server-auth-runtime-trust.md)

## Ownership Model

Adapters are compatibility shims.

They may own:

- transport contracts such as HTTP, WebSocket, gRPC, or custom wire protocols
- provider-specific runtime APIs and contract lowering
- provider-shaped identifiers, path parsing, request envelopes, and response
  shapes
- provider-specific tests, docs, and compatibility matrices

Adapters should not become the accidental shared home for reusable execution
primitives.

Those belong in:

- `nimbus-core` for zero-I/O types and validation
- `nimbus-engine` for canonical execution and mutation/query orchestration
- `nimbus-storage` for persistence and durable transaction semantics
- `provider_family/*` for shared provider-family translation seams
- `runtime_host/*` for provider-neutral runtime capabilities

## Dependency Rules

- Adapter code may depend on shared primitives and provider-family seams.
- Shared primitives may not depend on adapter-owned types.
- Adapter-to-adapter imports are a smell; if two adapters need the same logic,
  lift it into a shared provider-family or runtime-host seam instead.
- Provider-specific names should stay adapter-owned unless a shared contract has
  already been deliberately extracted and renamed generically.

## Composition Rules

- Keep adapter composition roots thin.
- New behavior should land in concept-owned child modules instead of growing
  switchboards inline.
- Prefer names that describe owned concepts such as `transport`, `contract`,
  `auth`, `registry`, `runtime_api`, or `query`.
- Avoid generic catch-alls like `helpers`, `utils`, or `misc` when ownership is
  actually adapter-specific.

## Verification Expectations

Every adapter should carry evidence for its own public contract:

- focused protocol/transport tests
- compatibility docs that say what is covered and what is intentionally out of
  scope
- at least one end-to-end or smoke lane for the primary integration surface

When a shared primitive changes because of adapter work, verify the primitive
and the adapter-facing contract separately.

## Modularity Expectations

- Adapter-owned files under 1,500 lines are usually acceptable if they tell one
  coherent ownership story.
- Files from 1,500 through 1,999 lines need an explicit justification in the
  active owning plan.
- Files at 2,000 lines or more must be decomposed or justified as a strong
  ownership-based exception in the active plan.

The point is not to split mechanically. The point is to keep ownership obvious
enough that adding a second adapter does not force another architecture cleanup
wave just to understand where the real primitives live.
