# NFRC13 Closeout

Date: 2026-05-28
Authoring agent: Codex
Repository baseline: `e7e8b9d6`
Relevant Node lanes: Node20 `v20.20.2`, Node22 `v22.22.3`, Node24 `v24.16.0`, Node26 `v26.2.0`

## Git Status Summary

The worktree contains the completed NFRC0-NFRC13 implementation wave. The
NFRC13-specific changes add the final all-row verifier, archive the completed
plan, update proof and plan indexes, and repoint the machine-readable FaaS
profile at the archived plan.

## Files Changed

- Final verifier:
  `scripts/verify-node-faas-runtime-compatibility.sh`
- Plan archive and indexes:
  `docs/plans/archive/node-faas-runtime-compatibility-plan.md`,
  `docs/plans/README.md`,
  `docs/plans/proof/node-faas-runtime-compatibility/README.md`
- FaaS profile plan path:
  `docs/architecture/runtime/node-faas-compatibility-profile.json`
- Control proof:
  this proof file

## Closeout Contract

The final verifier enforces:

- the active plan was archived and every NFRC row is `done`,
- all NFRC0-NFRC13 proof files exist and are listed,
- the FaaS profile points to the archived plan and keeps Node24 default,
  Node22 supported LTS, Node20 legacy-grace, and Node26 Current/non-LTS,
- generated public docs still distinguish supported, diagnostic, and
  service/microVM-routed behavior,
- release-train proof digests are current,
- PR/nightly CI wiring still gates the right lanes,
- fixture provenance, latest-suite, docs, release-train, watchpoint, canary,
  runtime, tenant, bridge, Convex, formatting, Markdown refs, and diff
  whitespace gates pass.

## Verification

Final verification commands:

- `cargo fmt --all --check`
- `npm run docs:validate-refs:strict`
- `bash scripts/verify-node-faas-runtime-compatibility.sh`: pass, 26 checks and
  0 failures.
- `git diff --check`

`bash scripts/verify-node-faas-runtime-compatibility.sh` is the authoritative
closeout gate; it runs the broad Application and Tooling canary presets as
well as the focused metadata and policy tests.

## Decisions

- Archive the plan only after the verifier exists and the proof ledger is
  complete.
- Keep the research and proof directories in their current locations; only the
  execution plan moves to `docs/plans/archive/`.
- Keep future Node runtime work on a new active plan rather than reviving this
  archived baseline.
