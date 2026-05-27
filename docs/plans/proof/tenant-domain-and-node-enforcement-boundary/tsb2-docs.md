# TSB2 Docs

Date: 2026-05-27

## Status

Status: `done`

## Git Base

- Branch: `main`
- Base revision: `9f8e8edc`

## Files Touched

- `docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
- `docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb2-docs.md`
- `docs/tenant-isolation.md`
- `docs/operating/tenant-isolation.md`
- `ARCHITECTURE.md`

## Requirement IDs Touched

- `REQ-ADMIT`: docs must keep `TenantIsolationDecision` as the admitted
  authority artifact required by runtime, HostBridge, sandbox, storage/API,
  node-local lifecycle, credentials, and system-tenant evidence.
- `REQ-SYSTEM`: docs must keep `_nimbus` operator/system-owned, not an
  application tenant API.
- `REQ-STORAGE`: docs must keep storage/API enforcement tied to tenant
  projections and stable table identity.
- `REQ-DOCS`: docs, plan state, proof notes, and exact verification commands
  must remain consistent with the TSB1 module rename.

## Behavior Changed

None. This is a documentation-only phase.

## Tests Added Or Updated

None.

## Verification Commands

To run before closing:

```sh
git diff --check -- ARCHITECTURE.md docs/tenant-isolation.md docs/operating/tenant-isolation.md docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb2-docs.md
npm run docs:validate-refs:strict
```

The TSB1 post-rename code tests remain the behavioral evidence for the module
rename. TSB2 only updates docs to match that code state.

Results:

- `git diff --check -- ARCHITECTURE.md docs/tenant-isolation.md docs/operating/tenant-isolation.md docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb2-docs.md`: passed with no output.
- `npm run docs:validate-refs:strict`: `docs reference validation: pass (212 working-tree Markdown files)`.

## Remaining Risks

- Broader local-enforcement, host-lifecycle, credential, and node-status docs
  still belong to TSB3 and later.
- The old `tenant_isolation` strings that name test filters, event schemas,
  policy fields, and drift APIs intentionally remain.

## Next Resumable Action

Commit the TSB2 docs checkpoint, then start TSB3 by adding the local
enforcement design doc and mapping the comparative patterns to Nimbus
components.
