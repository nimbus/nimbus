# NFRC1 FaaS Compatibility Profile

Date: 2026-05-28
Authoring agent: Codex
Baseline commit: `e7e8b9d6`

## Git Status Summary

NFRC work remains uncommitted. The pre-existing unrelated dirty file is still
present and untouched:

```text
 M docs/plans/dynamodb-adapter-plan.md
```

## Files Changed

- `docs/architecture/runtime/node-faas-compatibility-profile.json`
- `docs/architecture/runtime/node-faas-compatibility-profile.md`
- `tests/runtime/node/schemas/node-faas-compatibility-profile.schema.json`
- `scripts/runtime/node/faas_profile.py`
- `scripts/verify-node-faas-compat-profile.sh`
- `docs/architecture/runtime/node-compat-surface-matrix.md`
- `docs/runtimes/nodejs/compatibility.md`
- `tests/runtime/node/README.md`
- `docs/plans/node-faas-runtime-compatibility-plan.md`
- `docs/plans/proof/node-faas-runtime-compatibility/nfrc1-faas-compat-profile.md`

## Decisions

- Added a canonical FaaS compatibility profile JSON under architecture docs so
  runtime support statuses are data, not prose.
- Defined exactly six public support statuses: supported in-process,
  supported local-dev only, service/microVM required, import-compatible stub,
  unsupported, and not applicable to FaaS.
- Added version roles for Node20, Node22, Node24, and Node26. Node24 is the
  target product default after NFRC; Node26 is Current/non-LTS, not preview or
  LTS.
- Added API-family and package-class records with doc-generation fields,
  evidence references, and verification states.
- Added a validator that rejects unknown support statuses, unknown
  verification states, unknown evidence refs, missing evidence paths,
  evidence-free doc claims, missing generated-doc source arrays, and disabled
  wide-then-focused strategy flags.
- Added negative self-tests so the verifier proves the validator catches the
  most trust-critical mistakes rather than merely accepting the current file.

## Wide-Then-Focused Note

NFRC1 creates schema/profile control-plane data and does not touch runtime
behavior, fixture corpora, classifications, canaries, or package execution.
The broad-runtime-run phase is therefore not applicable to this row. The row
still makes the wide-then-focused strategy machine-readable and verifies that
future rows cannot disable it in the profile.

## Verification

```bash
bash scripts/verify-node-faas-compat-profile.sh
```

Result: pass. Output summary:

```text
validated Node FaaS compatibility profile: 6 statuses, 4 lanes, 11 API families, 7 package classes, 4 doc claims
Node FaaS compatibility profile negative self-tests passed
```

```bash
python3 scripts/runtime/node/schema.py validate --schema node-faas-compatibility-profile.schema.json --instance docs/architecture/runtime/node-faas-compatibility-profile.json
```

Result: pass.

```bash
npm run docs:validate-refs:strict
```

Result: pass, 224 working-tree Markdown files.

```bash
bash scripts/verify-node-lts-docs.sh
```

Result: pass. Output summary:

```text
Node.js runtime evidence docs are current in docs/runtimes/nodejs/evidence
Node LTS docs guard passed: hand-written docs avoid stale pass-rate and support-priority claims
```

```bash
git diff --check
```

Result: pass.

## Remaining Risks

- NFRC2 must connect latest Node release tags to the lane registry and fixture
  provenance gates.
- NFRC4 and NFRC5 must apply the wide-then-focused loop to large official
  fixture corpora.
- NFRC10 must turn the profile's doc-generation targets into actual generated
  Deno-style reference docs.
