# TSB3 Local Enforcement Design

Date: 2026-05-27

## Status

Status: `done`

## Git Base

- Branch: `main`
- Base revision: `3c16217a`

## Files Touched

- `docs/architecture/server/local-enforcement-boundary.md`
- `docs/architecture/README.md`
- `docs/README.md`
- `docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
- `docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb3-local-enforcement.md`

## Requirement IDs Touched

- `REQ-ADMIT`: the design makes `TenantIsolationDecision` or a narrow
  decision-derived binding the required input for runtime, sandbox, HostBridge,
  storage/API, credential, host lifecycle, and system evidence paths.
- `REQ-SYSTEM`: the design keeps `_nimbus` operator/system-owned and models
  all-tenant and cross-tenant targets as read-only by default.
- `REQ-STORAGE`: the design requires storage projections, stable `TableId`
  resolution, and explicit user/system/virtual/hidden/orphaned namespace
  selection.
- `REQ-STATUS`: the design specifies the node-status authorizer shape and
  denies status mutation of desired state, policy, grants, quota, placement,
  credentials, deletion authority, admission, and user data.
- `REQ-CREDS`: the design requires workload/audience/node-scoped credential
  projection and fail-closed checks for missing grant, wrong audience, wrong
  node, stale generation, wrong invocation, echo-back subjects, and missing
  redaction metadata.
- `REQ-LIFECYCLE`: the design classifies static controls as recreate-required,
  allows dynamic reload only with active projection re-checks, and requires
  last-known-good behavior for invalid dynamic updates.
- `REQ-TRUST`: the design requires runtime/sandbox/pool reuse to be monotonic
  in trust and stricter reuse after broader exposure to require teardown.
- `REQ-DOCS`: docs, indexes, plan status, and proof notes must stay consistent
  with the new local enforcement architecture reference.

## Behavior Changed

None. This is a documentation-only phase.

## Tests Added Or Updated

None. TSB3 defines the design and verification expectations for later behavior
phases; no Rust or JavaScript behavior changed.

## Verification Commands

To run before closing:

```sh
git diff --check -- docs/architecture/server/local-enforcement-boundary.md docs/architecture/README.md docs/README.md docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb3-local-enforcement.md
npm run docs:validate-refs:strict
```

Results:

- `git diff --check -- docs/architecture/server/local-enforcement-boundary.md docs/architecture/README.md docs/README.md docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb3-local-enforcement.md`: passed with no output.
- `npm run docs:validate-refs:strict`: `docs reference validation: pass (213 working-tree Markdown files)`.

## Remaining Risks

- TSB4 and later must turn the documented boundary into types and tests.
- This phase does not yet add `local_enforcement`, status authorization,
  credential projection, host lifecycle, or trust-class code.

## Next Resumable Action

Commit the TSB3 docs checkpoint, then start TSB4 by introducing the in-server
`local_enforcement` module only if the dependency shape reduces coupling.
