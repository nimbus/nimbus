# Convex-Compatible Contributor Guidelines

Read this file before changing `packages/convex/`, `examples/convex/`, or a
Convex-compatible API surface. These rules describe Nimbus's current contracts.
They are not a claim of complete upstream Convex parity.

## Start from repository truth

Use these sources in order:

1. [`../../../developers/convex/`](../../../developers/convex/) for supported
   application workflows.
2. [`../../../reference/convex/compatibility.md`](../../../reference/convex/compatibility.md)
   for supported and unsupported behavior.
3. [`../../../reference/convex/usage-rules.md`](../../../reference/convex/usage-rules.md)
   for authoring constraints.
4. [`../../../source-map.md`](../../../source-map.md) for the implementation
   behind public claims.
5. `crates/nimbus-convex/`, `crates/nimbus-server/src/adapters/`,
   `crates/nimbus-bridge/`, and focused tests for current behavior.

Do not rely on remembered upstream behavior when the compatibility reference
or source differs. If implementation and public documentation disagree, fix
the owning defect and its tests. Do not silently broaden a claim.

## Package and code-generation ownership

- `packages/nimbus` is the canonical first-party JavaScript implementation.
- `packages/convex` is the drop-in compatibility package. When behavior is the
  same, use a typed adapter, alias, or re-export. Do not copy the same logic
  into both packages.
- Application functions import `query`, `mutation`, `action`, and their
  internal variants from `./_generated/server`. Clients use references from
  `./_generated/api`.
- Treat `_generated/` and `.nimbus/` as generated output. Change the codegen
  templates or compiler that owns an output. Do not patch generated files as
  the implementation.
- Keep NodeNext import suffixes and runtime-only binding recognition intact
  when changing codegen.

## Function contracts

- Declare argument validators for public and internal functions.
- Queries are deterministic reads. They cannot write, schedule work, or use
  network I/O. Nimbus pins their time and seeds their pseudorandom stream for
  the invocation. They do not read the live host clock or entropy source.
- Mutations read and write through the engine transaction and can schedule
  work. They use the same deterministic runtime contract. All writes from one
  function invocation remain one execution-unit commit.
- Actions do not receive direct database access. They call functions through
  `ctx.runQuery`, `ctx.runMutation`, or `ctx.runAction`.
- Use a top-level `"use node"` directive only for action modules that need the
  Node runtime. Do not move queries or mutations into that profile to gain
  ambient capabilities.
- Use declared indexes for selective queries. Do not add broad filter scans
  when an index is the stable data-model boundary.
- A schema is optional. If present, it adds validation. Code must still
  preserve the repository invariant that an unschematized table accepts
  documents.

## Mutation and runtime boundaries

Every client document mutation must converge on one of three engine-owned
paths: the queued journal path, `apply_mutation_with_mode*` direct path, or
`MutationExecutionUnit`. Runtime host operations go through `HostBridge` and
`nimbus-bridge`. They do not write storage directly. An audit or change to
mutation behavior must name and test all three paths.

Keep document writes, supporting index changes, and the commit-log append in
one storage transaction. Handle ambiguous durable outcomes consistently across
all three paths. Do not create adapter-specific persistence or retry authority.

## Silo, tenant, and authentication trust

- A deployment URL contains a silo segment, but the segment is a selector for
  a server-provisioned verifier. It is not proof of tenant authority.
- Select the verifier bound to the URL silo, and then examine the bearer token.
  Do not derive a trusted silo from caller-controlled token claims, issuer,
  subject, headers, or request data.
- A production deploy with Convex artifacts requires an explicit
  `--convex-silo` or `NIMBUS_CONVEX_SILO` binding.
- The policy fails closed for anonymous access. The operator must bind the
  requested silo and anonymous access to the same team. Reserved Nimbus silos
  never enter this policy.
- Preserve the independent Cloud Functions trusted-tenant boundary when a
  shared adapter or deployment change touches both surfaces. A request path is
  never tenant authority.
- Tests must cover the correct silo, wrong silo, unprovisioned silo, invalid
  bearer, and anonymous policy cases when authentication behavior changes.

## Tests and documentation

Put protocol semantics tests with `nimbus-convex`, transport tests with the
server adapter, host-call tests at the bridge/engine seam, and package tests in
`packages/convex`. Use an end-to-end application test only for behavior that
crosses those boundaries.

For a behavior change:

1. Add a fail-before test at the owning seam.
2. Run focused package or crate tests.
3. Verify all affected mutation and trust paths.
4. Update the compatibility table and source map if the public claim changes.
5. Run the public documentation gates for changed pages.

Never weaken tenant checks, validator behavior, unsupported-surface failures,
or test assertions to match an upstream API shape.
