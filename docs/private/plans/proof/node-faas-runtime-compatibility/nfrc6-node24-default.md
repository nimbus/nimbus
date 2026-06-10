# NFRC6 Node24 Product Default

Date: 2026-05-28
Authoring agent: Codex
Repository baseline: `e7e8b9d6`

## Git Status Summary

The worktree contains NFRC0-NFRC6 Node FaaS compatibility changes, the
previous NFRC4/NFRC5 vendored Node fixture corpus updates, generated evidence
updates, and one unrelated pre-existing edit to
`docs/plans/dynamodb-adapter-plan.md`.

## Files Changed

- Node lane registry and manifests:
  `docs/architecture/runtime/node-lts-compat/node-lts-lanes.json`,
  `docs/architecture/runtime/node-lts-compat/node-lts-lanes.md`,
  `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/lanes/node22.json`,
  `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/lanes/node24.json`
- Runtime, tenant, bridge, and Convex default-selection tests and metadata:
  `crates/nimbus-runtime/src/limits/`, `crates/nimbus-runtime/src/runtime/tests/node/`,
  `crates/nimbus-tenant/src/operator_policy*`,
  `crates/nimbus-convex/src/registry/resolution/runtime_access.rs`
- Codegen defaults and runtime metadata:
  `packages/codegen/src/project_config.mjs`,
  `packages/codegen/src/runtime_metadata.mjs`,
  `packages/codegen/src/selftest/*.mjs`
- Watchpoints, classifications, reports, generated evidence, and public docs:
  `tests/runtime/node/expectations/rust-watchpoints.json`,
  `tests/runtime/node/classifications/node20.json`,
  `tests/runtime/node/classifications/node22.json`,
  `tests/runtime/node/classifications/node24.json`,
  `docs/architecture/runtime/node-compat-evidence/latest/`,
  `docs/runtimes/nodejs/`,
  `docs/architecture/runtime/node-lts-compat/`

## Strategy

NFRC6 followed the same wide-then-focused loop:

1. Flip the canonical role data first: Node24 becomes product default; Node22
   remains a supported Maintenance LTS lane; Node20 remains legacy-grace; Node26
   remains Current/non-LTS.
2. Run broad status/dashboard generation to expose stale role assumptions. The
   first broad status run failed with catalog reason mismatches, and the first
   dashboard audit found stale report artifacts still referring to old
   `node22_default_lane`, `node24_supported_lane`, and `node20_supported_lane`
   test names.
3. Fix the focused causes: source watchpoint wording, generated watchpoint
   catalogs, classifications, representative report artifacts, hand-written
   node-lts docs, and deterministic Rust test expectations.
4. Rerun broad status, inventory, dashboard, evidence, docs, and trust gates
   before closing the row.

## Decisions

- Node24 is now the product default because it is the current Active LTS lane.
  This changes routing/config defaults only; it does not make official-suite
  pass rate an enterprise support percentage.
- Node22 remains a supported Maintenance LTS lane with lane-local evidence and
  supported canaries.
- Node26 stays Current/non-LTS. Codegen accepts explicit `nodeVersion: "26"`
  because the runtime has a real Current compatibility target, but generated
  docs and registry data keep it out of supported-LTS default behavior.
- Node20 stays legacy-grace/EOL and is labeled as such in watchpoint language,
  canary docs, and generated evidence.

## Wide Feedback And Focused Fixes

Initial NFRC6 status after the registry/default swap:

```bash
python3 scripts/runtime/node/status.py --output-root target/node-compat/status-nfrc6-default-swap
```

Result: failed with five catalog reason mismatches caused by stale Node24
supported-lane watchpoint wording.

Focused fixes:

```bash
python3 scripts/runtime/node/watchpoints.py sync
python3 scripts/runtime/node/classifications.py sync --lane node20
python3 scripts/runtime/node/classifications.py sync --lane node22
python3 scripts/runtime/node/classifications.py sync --lane node24
python3 scripts/runtime/node/classifications.py sync --lane node26
bash scripts/runtime/node/report.sh --family networking --slice dns-net-foundation
```

The report regeneration rebuilt the representative networking slice and catalog
artifacts instead of hand-editing generated JSON.

Final status and inventories:

```bash
python3 scripts/runtime/node/status.py --output-root target/node-compat/status-nfrc6-default-swap
python3 scripts/runtime/node/inventory.py --lane node20 --output-root target/node-compat/inventory-nfrc6-default-swap
python3 scripts/runtime/node/inventory.py --lane node22 --output-root target/node-compat/inventory-nfrc6-default-swap
python3 scripts/runtime/node/inventory.py --lane node24 --output-root target/node-compat/inventory-nfrc6-default-swap
python3 scripts/runtime/node/inventory.py --lane node26 --output-root target/node-compat/inventory-nfrc6-default-swap
```

Result: pass for all lanes, zero warnings, zero unclassified fixtures.

Published evidence now reports:

| Lane | Role | Upstream | Vendored | Passed | Expected failure / known gap | Skipped / excluded | Unclassified |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| Node20 | `legacy` | `v20.20.2` | 1,308 | 902 | 401 | 5 | 0 |
| Node22 | `supported` | `v22.22.3` | 4,748 | 1,000 | 3,728 | 20 | 0 |
| Node24 | `default` | `v24.16.0` | 5,198 | 1,002 | 4,149 | 47 | 0 |
| Node26 | `current` | `v26.2.0` | 5,578 | 0 | 5,529 | 49 | 0 |

## Generated Evidence And Docs

Regeneration commands:

```bash
python3 scripts/runtime/node/status.py
python3 scripts/runtime/node/dashboard.py
python3 scripts/runtime/node/trends.py
python3 scripts/runtime/node/publish_evidence.py
python3 scripts/runtime/node/publish_docs.py
```

Results:

- `docs/architecture/runtime/node-compat-evidence/latest/status-summary.md`
  labels Node24 as `default` and Node22 as `supported`.
- `docs/runtimes/nodejs/evidence/latest.md` labels Node24 as
  `Product default; Active LTS` and Node22 as
  `Supported; Maintenance LTS`.
- A role-wording audit over non-archived code/docs found no remaining
  `node22_default_lane`, `node24_supported_lane`, `node20_supported_lane`,
  `Node24 supported-lane`, or `Node20 supported-lane` references.

## Verification

- `bash scripts/verify-node-lts-lanes.sh`: pass, 4 lanes, product default
  `node24`, consumers `nimbus-runtime`, `nimbus-tenant`, `nimbus-bridge`, and
  `nimbus-convex`.
- `python3 scripts/runtime/node/fixture_provenance.py validate`: pass, 4
  vendored corpora and 2 supported LTS lanes with zero unclassified published
  results.
- `bash scripts/verify-node-latest-suite-tags.sh`: pass, 4 lanes, 0 needing
  fixture sync; negative self-tests passed.
- `NIMBUS_ENFORCE_CURRENT_NODE_CORPORA=1 bash scripts/verify-node-latest-suite-tags.sh`:
  pass, all targeted Node fixture corpora are current.
- `bash scripts/verify-node-lts-docs.sh`: pass, generated Node.js evidence
  docs are current and stale prose guard passed.
- `bash scripts/runtime/node/validate-claims.sh`: pass, 12 active claim
  mappings against 12 registered canaries.
- `npm run docs:validate-refs:strict`: pass, 226 working-tree Markdown files.
- `cargo test -p nimbus-runtime node_compat_lane_metadata -- --nocapture`:
  pass, 3 tests.
- `cargo test -p nimbus-runtime node_compat_manifest_resolution -- --nocapture`:
  pass, 7 tests.
- `cargo test -p nimbus-runtime node_compat_manifest_topology -- --nocapture`:
  pass after adding `node26.json` to the deterministic lane-file contract, 17
  tests.
- `cargo test -p nimbus-runtime node_compat_manifest_report -- --nocapture`:
  pass after updating deterministic report expectations for Node22 supported /
  Node24 default, 11 passed and 1 ignored manual report entrypoint.
- `cargo test -p nimbus-runtime node_lts -- --nocapture`: pass, 3 tests.
- `cargo test -p nimbus-tenant node_runtime_profiles_follow_lts_registry_targets -- --nocapture`:
  pass, 1 test.
- `cargo test -p nimbus-convex convex_node_runtime_lanes_follow_lts_registry_targets -- --nocapture`:
  pass, 1 test.
- `npm run test`: pass. The UI Vitest lane reported 42 test files and 278
  tests passed; package selftests and codegen selftests exited successfully.
- `bash scripts/verify-node-lts-runtime-trust.sh`: pass, 16 verifier sections.

## Remaining Risks

- NFRC7 still owns real Convex `"use node"` app flows beyond the existing
  packaging/canary lane coverage.
- NFRC8 still owns broader SDK/package canaries.
- NFRC9 still owns host-heavy negative canaries and diagnostics.
- NFRC10 still owns Deno-style generated API/package reference docs.
