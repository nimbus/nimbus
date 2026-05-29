# NFRC0 Baseline And Control Plane

Date: 2026-05-28
Authoring agent: Codex
Baseline commit: `e7e8b9d6`

## Git Status Summary

At activation time, the working tree includes NFRC plan edits plus one
pre-existing unrelated dirty file:

```text
 M docs/plans/README.md
 M docs/plans/dynamodb-adapter-plan.md
?? docs/plans/node-faas-runtime-compatibility-plan.md
?? docs/plans/proof/node-faas-runtime-compatibility/README.md
?? docs/plans/research/node-faas-runtime-compatibility-2026.md
```

`docs/plans/dynamodb-adapter-plan.md` is unrelated and must not be reverted or
included in NFRC proof.

## Files Changed

- `docs/plans/node-faas-runtime-compatibility-plan.md`
- `docs/plans/research/node-faas-runtime-compatibility-2026.md`
- `docs/plans/proof/node-faas-runtime-compatibility/README.md`
- `docs/plans/proof/node-faas-runtime-compatibility/nfrc0-baseline-and-control-plane.md`
- `docs/plans/README.md`

## Owner-Crate Map

- `nimbus-runtime` owns the Node lane registry, runtime target metadata,
  process metadata, runtime limits presets, node-compat fixtures, app canaries,
  classifications, dashboards, and generated developer evidence.
- `nimbus-tenant` consumes the registry for tenant/operator runtime profile
  mapping and production admission policy.
- `nimbus-bridge` consumes selected lanes at execution admission and fallback
  routing boundaries.
- `nimbus-convex` consumes the registry for Convex-compatible manifest runtime
  selection and `"use node"` action packaging/routing.

## Decisions

- Activated the plan from `ready` to `active`.
- Kept Node26's official release phase as Current, not preview.
- Kept Node26's Nimbus support promise separate as Current/non-LTS until Node
  promotes it to LTS and Nimbus supported-LTS gates pass.
- Added the wide-then-focused compatibility loop as a control-plane rule:
  broad corpus first, issue inventory second, isolated fixes third, final broad
  rerun last.
- Preserved the completed NLRT baseline as the required prior context instead
  of duplicating its lane-registry and Deno-fork details.

## Verification

```bash
npm run docs:validate-refs:strict
```

Result: pass, 223 working-tree Markdown files.

```bash
git diff --check
```

Result: pass.

## Remaining Risks

- NFRC1 must turn the support statuses and wide-run inventory fields into a
  machine-readable manifest/schema so later rows cannot drift into prose-only
  claims.
- NFRC4 and NFRC5 are the first rows that will stress the wide-then-focused
  loop against large official fixture corpora.
