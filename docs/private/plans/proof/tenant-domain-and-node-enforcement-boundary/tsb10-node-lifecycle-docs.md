# TSB10 Node Lifecycle Docs

Date: 2026-05-27

## Status

Status: `done`

## Git Base

- Branch: `main`
- Base revision: `463bc9c3`

## Files Touched

- `docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
- `docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb10-node-lifecycle-docs.md`
- `docs/operating/node-lifecycle.md`
- `docs/README.md`
- `docs/operating/cli.md`
- `docs/operating/container-image.md`
- `docs/architecture/server/local-enforcement-boundary.md`
- `docs/architecture/sandbox/macos-machine-flow.md`

## Requirement IDs Touched

- `REQ-ARTIFACT`: docs now distinguish native systemd node units,
  containerized Quadlet node install, static Quadlet export, and `machine-os`
  baked units as separate operator surfaces.
- `REQ-HOST`: docs state dynamic tenant workloads use
  `SystemdTransientUnitBackend` over systemd D-Bus transient units, not
  Quadlet and not `systemd-run` in product code.
- `REQ-LIFECYCLE`: docs include local development, native node,
  containerized node, `machine-os`, dynamic workload, static export, and
  direct-process fallback lifecycle ownership.
- `REQ-DOCS`: plan state and this proof note record exact files, commands,
  result counts, risks, and the next phase.

## Behavior Changed

Documentation changed intentionally:

- Added `docs/operating/node-lifecycle.md` with a lifecycle decision matrix,
  Mermaid control-flow diagram, local development flow, native Linux node flow,
  containerized Quadlet node flow, `machine-os` guidance, dynamic tenant
  workload guidance, static Quadlet export guidance, and troubleshooting
  commands.
- Linked the new runbook from `docs/README.md`, `docs/operating/cli.md`,
  `docs/operating/container-image.md`,
  `docs/architecture/server/local-enforcement-boundary.md`, and
  `docs/architecture/sandbox/macos-machine-flow.md`.
- Updated the CLI reference with `nimbus node ...` and
  `nimbus compose export quadlet` command shapes.

## Verification Commands

Commands run:

```sh
npm run docs:validate-refs:strict
git diff --check -- docs/operating/node-lifecycle.md docs/README.md docs/operating/container-image.md docs/operating/cli.md docs/architecture/server/local-enforcement-boundary.md docs/architecture/sandbox/macos-machine-flow.md docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb10-node-lifecycle-docs.md
rg -n "nimbus node install|compose export quadlet|SystemdTransientUnitBackend|DirectProcessBackend|machine-os|systemd-run|containerized node" docs/operating/node-lifecycle.md docs/operating/cli.md docs/operating/container-image.md docs/architecture/server/local-enforcement-boundary.md docs/architecture/sandbox/macos-machine-flow.md
```

Results:

- `npm run docs:validate-refs:strict`: `docs reference validation: pass (211
  working-tree Markdown files)`.
- `git diff --check -- ...`: passed with no output.
- `rg -n ...`: returned references in the new runbook and linked docs for
  `nimbus node install`, `compose export quadlet`,
  `SystemdTransientUnitBackend`, `DirectProcessBackend`, `machine-os`,
  `systemd-run`, and containerized node lifecycle wording.

## Remaining Risks

- This is documentation-only. Live Linux service-manager and Podman execution
  remain covered by TSB8's residual risk and future Linux integration lanes.
- The runbook gives operator commands and boundaries, but TSB11 still needs
  richer lifecycle evidence IDs in status, audit, diagnostics, and `_nimbus`
  records.

## Next Resumable Action

Commit the TSB10 node lifecycle docs checkpoint, then start TSB11 by wiring host
lifecycle status, unit/job/process IDs, cgroup paths, journal selectors,
decision IDs, observed generation, backend capability detection, and `_nimbus`
evidence correlation into status, audit, diagnostics, and system tenant records.
