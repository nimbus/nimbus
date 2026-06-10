# NFRC5 Node26 and Refreshed Classification

Date: 2026-05-28
Authoring agent: Codex
Repository baseline: `e7e8b9d6`

## Git Status Summary

The worktree contains NFRC0-NFRC5 Node FaaS compatibility changes, large
vendored Node fixture corpus updates for Node22, Node24, and Node26, generated
classification/evidence updates, and one unrelated pre-existing edit to
`docs/plans/dynamodb-adapter-plan.md`.

## Files Changed

- `tests/runtime/node/classifications/node22.json`
- `tests/runtime/node/classifications/node24.json`
- `tests/runtime/node/classifications/node26.json`
- `docs/architecture/runtime/node-compat-evidence/latest/README.md`
- `docs/architecture/runtime/node-compat-evidence/latest/status-summary.json`
- `docs/architecture/runtime/node-compat-evidence/latest/status-summary.md`
- `docs/architecture/runtime/node-compat-evidence/latest/dashboard-summary.json`
- `docs/architecture/runtime/node-compat-evidence/latest/dashboard-summary.md`
- `docs/architecture/runtime/node-compat-evidence/latest/trend-summary.json`
- `docs/architecture/runtime/node-compat-evidence/latest/trend-summary.md`
- `docs/runtimes/nodejs/evidence/README.md`
- `docs/runtimes/nodejs/evidence/latest.md`
- `docs/runtimes/nodejs/evidence/node20.md`
- `docs/runtimes/nodejs/evidence/node22.md`
- `docs/runtimes/nodejs/evidence/node24.md`
- `docs/runtimes/nodejs/evidence/node26.md`
- `docs/plans/node-faas-runtime-compatibility-plan.md`
- `docs/plans/proof/node-faas-runtime-compatibility/README.md`

## Strategy

NFRC5 deliberately followed the wide-then-focused loop required by the plan:

1. NFRC4 first vendored the latest broad official corpora and ran wide status
   and inventory commands. That exposed the complete issue inventory instead of
   spending cycles on isolated green tests.
2. NFRC5 then synchronized classification catalogs for the affected lanes.
   There were no runtime behavior changes in this row; the wide result showed
   that the remaining work was classification and support-boundary labeling.
3. Final wide status, inventory, dashboard, trend, evidence, and docs
   generation were rerun before the row was closed.

## Initial Wide Inventory

The NFRC4 handoff status run was:

```bash
python3 scripts/runtime/node/status.py --output-root target/node-compat/status-nfrc4-initial
```

It intentionally exposed unclassified remainders after the corpus refresh:

| Lane | Vendored test files | Passed path-owned fixtures | Classified red/skip | Unclassified |
| --- | ---: | ---: | ---: | ---: |
| Node22 | 4,748 | 1,000 | 408 | 3,340 |
| Node24 | 5,198 | 1,002 | 573 | 3,623 |
| Node26 | 5,578 | 0 | 0 | 5,578 |

The supported-LTS provenance guard correctly failed against that snapshot:

```bash
python3 scripts/runtime/node/fixture_provenance.py validate --status-summary target/node-compat/status-nfrc4-initial/status-summary.json
```

Expected failure summary:

- Node22 had 3,340 unclassified published fixtures.
- Node24 had 3,623 unclassified published fixtures.

That failure was the desired broad feedback loop for NFRC5.

## Focused Classification Commands

Classification catalogs were generated lane by lane:

```bash
python3 scripts/runtime/node/classifications.py sync --lane node22
python3 scripts/runtime/node/classifications.py sync --lane node24
python3 scripts/runtime/node/classifications.py sync --lane node26
```

The resulting catalog summary:

| Lane | Expected failure | Known gap | Skipped / excluded | Unpromoted surface | Watchpoints |
| --- | ---: | ---: | ---: | ---: | ---: |
| Node22 | 34 | 3,694 | 20 | 3,417 | 34 |
| Node24 | 33 | 4,116 | 47 | 3,825 | 33 |
| Node26 | 0 | 5,529 | 49 | 5,233 | 0 |

Node26 is a Current/non-LTS compatibility target. Its known-gap
classifications do not lower supported-LTS claims because the lane role remains
`current`, the supported-LTS provenance guard only gates Node22/Node24, and
published dashboards label the Node26 lane separately from default/supported
LTS lanes.

## Final Wide Result

Fresh status:

```bash
python3 scripts/runtime/node/status.py --output-root target/node-compat/status-nfrc5-classified
```

Result: pass, no warnings.

| Lane | Role | Upstream | Vendored | Passed | Expected failure / known gap | Skipped / excluded | Classified coverage count | Unclassified | Pass rate |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Node20 | `legacy` | `v20.20.2` | 1,308 | 901 | 402 | 5 | 1,308 | 0 | 68.9% |
| Node22 | `default` | `v22.22.3` | 4,748 | 1,000 | 3,728 | 20 | 4,748 | 0 | 21.1% |
| Node24 | `supported` | `v24.16.0` | 5,198 | 1,002 | 4,149 | 47 | 5,198 | 0 | 19.3% |
| Node26 | `current` | `v26.2.0` | 5,578 | 0 | 5,529 | 49 | 5,578 | 0 | 0.0% |

Fresh inventory:

```bash
python3 scripts/runtime/node/inventory.py --lane node22 --output-root target/node-compat/inventory-nfrc5-classified
python3 scripts/runtime/node/inventory.py --lane node24 --output-root target/node-compat/inventory-nfrc5-classified
python3 scripts/runtime/node/inventory.py --lane node26 --output-root target/node-compat/inventory-nfrc5-classified
```

Result: pass for all three lanes, zero warnings, zero unclassified fixtures,
and zero passed reconstructability gaps.

Default status/inventory evidence was regenerated:

```bash
python3 scripts/runtime/node/status.py
python3 scripts/runtime/node/inventory.py --lane node22
python3 scripts/runtime/node/inventory.py --lane node24
python3 scripts/runtime/node/inventory.py --lane node26
python3 scripts/runtime/node/dashboard.py
python3 scripts/runtime/node/trends.py
python3 scripts/runtime/node/publish_evidence.py
python3 scripts/runtime/node/publish_docs.py
```

Published evidence now reports zero unclassified fixtures for Node20, Node22,
Node24, and Node26.

## Verification

- `bash scripts/runtime/node/validate-claims.sh`: pass, 12 active claim
  mappings against 12 registered canaries.
- `python3 scripts/runtime/node/fixture_provenance.py validate`: pass, 4
  vendored corpora and 2 supported LTS lanes with zero unclassified published
  results.
- `bash scripts/verify-node-latest-suite-tags.sh`: pass, 4 lanes, 0 needing
  fixture sync; negative self-tests passed.
- `NIMBUS_ENFORCE_CURRENT_NODE_CORPORA=1 bash scripts/verify-node-latest-suite-tags.sh`:
  pass, all targeted Node fixture corpora are current.
- `bash scripts/verify-node-lts-lanes.sh`: pass, 4 lanes, product default
  `node22`, consumers `nimbus-runtime`, `nimbus-tenant`, `nimbus-bridge`, and
  `nimbus-convex`.
- `bash scripts/verify-node-lts-docs.sh`: pass, generated Node.js evidence
  docs are current and stale prose guard passed.
- `npm run docs:validate-refs:strict`: pass, 226 working-tree Markdown files.
- `cargo test -p nimbus-runtime node_compat_lane_metadata -- --nocapture`:
  pass, 3 tests.
- `cargo test -p nimbus-runtime node_compat_manifest_resolution -- --nocapture`:
  pass, 7 tests.
- `cargo fmt --all --check`: pass.
- `git diff --check`: pass.

## Decisions

- Kept Node26 as Current/non-LTS. It is included in corpus classification and
  generated evidence, but not promoted into supported-LTS pass gates.
- Classified broad official fixtures before adding more isolated canaries. This
  preserves the plan's feedback strategy: broad corpus first, focused work
  second, broad rerun before closure.
- Published generated evidence only after status and inventory both reported
  zero unclassified fixtures and zero warnings.

## Remaining Risks

- Node26 has no path-owned official fixture passes yet. NFRC7, NFRC8, and
  NFRC12 own Current-line canary execution and scheduled reporting.
- The low official-suite pass percentages are not product support percentages.
  NFRC10 must keep Deno-style public docs generated from evidence and must
  explain the difference between official fixture classification, FaaS profile
  support, and host-heavy unsupported/service-routed behavior.
