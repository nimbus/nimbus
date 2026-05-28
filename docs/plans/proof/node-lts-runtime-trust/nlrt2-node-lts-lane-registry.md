# NLRT2 Node LTS Lane Registry

Date: 2026-05-28
Authoring agent: Codex
Status: done

## Scope

Add a checked-in, data-driven Node LTS lane registry that separates support
phase, product default, upstream release identity, fixture corpus identity, and
evidence policy. The registry must make Node20 legacy/EOL, Node22 Maintenance
LTS, Node24 Active LTS, and Node26 preview/current without forcing the current
node-compat harness to claim a Node26 fixture corpus.

## Files Changed

- `docs/architecture/runtime/node-lts-compat/node-lts-lanes.json`
- `docs/architecture/runtime/node-lts-compat/node-lts-lanes.md`
- `docs/architecture/runtime/node-compat-surface-matrix.md`
- `tests/runtime/node/schemas/node-lts-lanes.schema.json`
- `scripts/runtime/node/lane_registry.py`
- `scripts/verify-node-lts-lanes.sh`
- `docs/plans/node-lts-runtime-trust-plan.md`
- `docs/plans/proof/node-lts-runtime-trust/README.md`
- `docs/plans/proof/node-lts-runtime-trust/nlrt2-node-lts-lane-registry.md`

## Upstream Facts Rechecked

Primary sources checked on 2026-05-28:

- Node release schedule JSON:
  `https://raw.githubusercontent.com/nodejs/Release/main/schedule.json`
- Node releases page: `https://nodejs.org/en/about/releases/`
- Node EOL page: `https://nodejs.org/en/about/eol`
- Node download page: `https://nodejs.org/en/download`

Recorded lane facts:

| Lane | Phase | Codename | Latest upstream tag | Fixture tag | LTS start | Maintenance start | EOL |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `node20` | `eol_legacy` | Iron | `v20.20.2` | `v20.20.2` | 2023-10-24 | 2024-10-22 | 2026-04-30 |
| `node22` | `maintenance_lts` | Jod | `v22.22.3` | `v22.15.0` | 2024-10-29 | 2025-10-21 | 2027-04-30 |
| `node24` | `active_lts` | Krypton | `v24.16.0` | `v24.15.0` | 2025-10-28 | 2026-10-20 | 2028-04-30 |
| `node26` | `preview_current` | unassigned | `v26.2.0` | none | 2026-10-28 | 2027-10-20 | 2029-04-30 |

The Node26 maintenance date is recorded from the current upstream schedule JSON
as 2027-10-20. That corrects the older research note that listed 2027-10-27.

## Decisions

- Added a dedicated registry at
  `docs/architecture/runtime/node-lts-compat/node-lts-lanes.json` instead of
  overloading the existing fixture lane manifests. The fixture manifests remain
  evidence-corpus inputs; the registry is the support and product contract.
- Kept Node26 in the registry with a `null` runtime target and fixture corpus.
  That lets operators and future agents see the upcoming lane without making
  today's runtime claim unsupported execution.
- Separated latest upstream release tags from fixture corpus tags. Node22 and
  Node24 are valid supported LTS lanes, but their checked-in fixture corpora are
  behind the latest release branch and will be handled by NLRT6 provenance/sync.
- Made Node22's product-default status a boolean flag, not an evidence policy.
  The supported LTS evidence policy is the same for Node22 and Node24.
- Named all consumer crates in the registry: `nimbus-runtime`,
  `nimbus-tenant`, `nimbus-bridge`, and `nimbus-convex`.

## Validation

`scripts/verify-node-lts-lanes.sh` validates the JSON schema and semantic
contract:

- required lanes are present exactly once;
- exactly one product default exists and matches `product_default_lane`;
- required owner crates include `nimbus-runtime`, `nimbus-tenant`, and
  `nimbus-convex`;
- Node20/22/24/26 have the expected support phases;
- supported LTS lanes use lane-local evidence policy;
- Node26 preview does not claim a runtime target or fixture corpus;
- Node20/22/24 fixture corpus path, fixture tag, and runtime target match the
  existing `node_compat_manifests/lanes/*.json` evidence manifests.

## Verification

```text
bash scripts/verify-node-lts-lanes.sh
validated Node LTS lane registry: 4 lanes, product default node22, consumers nimbus-runtime, nimbus-tenant, nimbus-bridge, nimbus-convex
```

```text
npm run docs:validate-refs:strict
docs reference validation: pass (219 working-tree Markdown files)
```

```text
git diff --check
pass
```

## Remaining Risks

- NLRT3 still needs to wire runtime target metadata to the registry and remove
  hard-coded synthetic version strings.
- NLRT5 still needs to de-center stale generated/prose surfaces that currently
  use older `lane_role` and `public_contract_role` language.
- NLRT6 still needs fixture provenance/sync automation for the lag between the
  latest Node22/Node24 release tags and the checked-in fixture corpus tags.
