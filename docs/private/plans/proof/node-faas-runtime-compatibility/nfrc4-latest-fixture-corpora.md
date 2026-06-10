# NFRC4 Latest Fixture Corpora

Date: 2026-05-28
Authoring agent: Codex
Repository baseline: `e7e8b9d6`

## Git Status Summary

The worktree contains NFRC0-NFRC4 Node FaaS compatibility changes, large
vendored Node fixture corpus updates for Node22, Node24, and Node26, and one
unrelated pre-existing edit to `docs/plans/dynamodb-adapter-plan.md`.

## Fixture Tags

| Lane | Corpus tag | Tag object | Commit | Test files after sync |
| --- | --- | --- | --- | ---: |
| Node22 | `v22.22.3` | `354ef4b9bd94d5b662a9c300ddacc67f95a1bbe8` | `fdfa0ff0dbaf0fbf4d7d6d89a2ab807f3177fa5c` | 4,748 |
| Node24 | `v24.16.0` | `75143a8d75629c5d429dd0becb0d725e955f48fb` | `c7d10158bc31036de6783d66beaaaf551e3167aa` | 5,198 |
| Node26 | `v26.2.0` | `30ffe3cfc2fda3684c38ec43aa79c381d398bf14` | `cfd7920d5a2d84905c4292362d01d07870047e93` | 5,578 |

Node20 remains unchanged at `v20.20.2` as legacy-grace regression coverage.

## Files Changed

- `crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/node22/test/`
- `crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/node24/test/`
- `crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/node26/test/`
- `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/lanes/node22.json`
- `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/lanes/node24.json`
- `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/lanes/node26.json`
- `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/schema.json`
- `crates/nimbus-runtime/src/runtime/tests/node/manifest_catalog.rs`
- `crates/nimbus-runtime/src/runtime/tests/node/manifest_metadata.rs`
- `crates/nimbus-runtime/src/runtime/tests/node/manifest_report.rs`
- `crates/nimbus-runtime/src/runtime/tests/node/manifest_resolution.rs`
- `crates/nimbus-runtime/src/runtime/tests/node/oracle.rs`
- `docs/architecture/runtime/node-lts-compat/node-lts-lanes.json`
- `docs/architecture/runtime/node-lts-compat/node-latest-suite-tags.json`
- `docs/architecture/runtime/node-lts-compat/node-latest-suite-tags.md`
- `docs/runtimes/nodejs/evidence/refreshing.md`
- `Makefile`
- `scripts/runtime/node/sync.py`
- `scripts/runtime/node/refresh.py`
- `scripts/runtime/node/lane_registry.py`
- `scripts/runtime/node/latest_suite_tags.py`
- `scripts/runtime/node/classifications.py`
- `scripts/runtime/node/status.py`
- `scripts/runtime/node/inventory.py`
- `scripts/validate-docs-refs.mjs`

## Decisions

- Used the canonical local Node checkout at
  `/Users/jack/src/github.com/nodejs/node` instead of network clones.
- Added `--source-root` to the fixture sync/refresh tooling so future agents can
  reproduce exact-tag fixture syncs from the canonical local checkout.
- Vendored the full upstream `test/` subtree for Node22, Node24, and Node26,
  not only the currently green subset. This intentionally creates a wide issue
  inventory before NFRC5 classification.
- Taught docs reference validation to ignore vendored
  `node_compat_fixtures/` Markdown. Those files are upstream test data with
  links relative to the full Node repository, not Nimbus docs.
- Kept published evidence pages unchanged for NFRC4. The fresh status run is
  an inventory artifact; NFRC5 owns classification and evidence publication.

## Sync Commands

Compare reports were generated before apply:

- `python3 scripts/runtime/node/sync.py --lane node22 --upstream-tag v22.22.3 --compare-upstream --source-root /Users/jack/src/github.com/nodejs/node`
- `python3 scripts/runtime/node/sync.py --lane node24 --upstream-tag v24.16.0 --compare-upstream --source-root /Users/jack/src/github.com/nodejs/node`
- `python3 scripts/runtime/node/sync.py --lane node26 --upstream-tag v26.2.0 --compare-upstream --source-root /Users/jack/src/github.com/nodejs/node`

Apply commands:

- `python3 scripts/runtime/node/sync.py --lane node22 --upstream-tag v22.22.3 --apply --source-root /Users/jack/src/github.com/nodejs/node`
- `python3 scripts/runtime/node/sync.py --lane node24 --upstream-tag v24.16.0 --apply --source-root /Users/jack/src/github.com/nodejs/node`
- `python3 scripts/runtime/node/sync.py --lane node26 --upstream-tag v26.2.0 --apply --source-root /Users/jack/src/github.com/nodejs/node`

Diff summaries from the compare/apply reports:

| Lane | Local test files before | Upstream test files | Added by upstream | Removed by upstream | Modified by upstream | Unchanged |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Node22 | 1,283 | 4,748 | 8,026 | 29 | 102 | 1,155 |
| Node24 | 1,495 | 5,198 | 8,668 | 0 | 23 | 1,480 |
| Node26 | 0 | 5,578 | 10,729 | 0 | 0 | 0 |

The `added_by_upstream` counts include all files under the upstream `test/`
subtree, not just `test-*` JS/CJS/MJS files.

## Initial Wide Inventory

Fresh status command:

```bash
python3 scripts/runtime/node/status.py --output-root target/node-compat/status-nfrc4-initial
```

It wrote `target/node-compat/status-nfrc4-initial/status-summary.{json,md}` and
exited `1` with two stale Node22 classification warnings:

- `test/parallel/test-fs-cp.mjs`
- `test/parallel/test-stream-readable-to-web.js`

Initial lane status:

| Lane | Vendored test files | Passed path-owned fixtures | Classified red/skip | Unclassified |
| --- | ---: | ---: | ---: | ---: |
| Node22 | 4,748 | 1,000 | 408 | 3,340 |
| Node24 | 5,198 | 1,002 | 573 | 3,623 |
| Node26 | 5,578 | 0 | 0 | 5,578 |

Inventory commands:

- `python3 scripts/runtime/node/inventory.py --lane node22 --output-root target/node-compat/inventory-nfrc4-initial`: pass with warning
  `rust_unreferenced_unclassified_count_differs_from_documented_green_reconstructability_gap`.
- `python3 scripts/runtime/node/inventory.py --lane node24 --output-root target/node-compat/inventory-nfrc4-initial`: pass with the same warning.
- `python3 scripts/runtime/node/inventory.py --lane node26 --output-root target/node-compat/inventory-nfrc4-initial`: pass with the same warning.

Largest unclassified buckets:

| Lane | Largest directory | Count | Largest prefix | Count |
| --- | --- | ---: | --- | ---: |
| Node22 | `test/parallel` | 2,527 | `http` | 365 |
| Node24 | `test/parallel` | 2,566 | `http` | 389 |
| Node26 | `test/parallel` | 4,401 | `http` | 453 |

## Guard Behavior

The latest-tag verifier now passes in normal and enforcement mode:

- `bash scripts/verify-node-latest-suite-tags.sh`: pass, 4 lanes, 0 needing fixture sync; negative self-tests passed.
- `NIMBUS_ENFORCE_CURRENT_NODE_CORPORA=1 bash scripts/verify-node-latest-suite-tags.sh`: pass; all targeted corpora current.

The supported-LTS provenance guard correctly fails when pointed at the fresh
unclassified NFRC4 status snapshot:

```bash
python3 scripts/runtime/node/fixture_provenance.py validate --status-summary target/node-compat/status-nfrc4-initial/status-summary.json
```

Expected output:

- Node22 has 3,340 unclassified published fixtures.
- Node24 has 3,623 unclassified published fixtures.

This is the intended handoff to NFRC5, not a green support claim.

## Verification

- `python3 scripts/runtime/node/fixture_provenance.py validate`: pass, 4
  vendored corpora and 2 supported LTS lanes with zero unclassified published
  results in the checked-in published baseline.
- `bash scripts/verify-node-lts-lanes.sh`: pass, 4 lanes, product default
  `node22`.
- `bash scripts/verify-node-latest-suite-tags.sh`: pass, 4 lanes, 0 needing
  fixture sync.
- `NIMBUS_ENFORCE_CURRENT_NODE_CORPORA=1 bash scripts/verify-node-latest-suite-tags.sh`:
  pass, all targeted corpora current.
- `cargo test -p nimbus-runtime node_compat_lane_metadata -- --nocapture`:
  pass, 3 tests.
- `cargo test -p nimbus-runtime node_compat_manifest_resolution -- --nocapture`:
  pass, 7 tests.
- `cargo fmt --all --check`: pass.
- `npm run docs:validate-refs:strict`: pass, 225 working-tree Markdown files.
- `bash scripts/verify-node-lts-docs.sh`: pass.
- `git diff --check`: pass.

## Remaining Risks

- NFRC5 must classify Node22, Node24, and Node26 fresh corpus remainders before
  fresh evidence can be published as green.
- Node26 has a corpus and metadata but still has zero path-owned green official
  fixtures. Current-line app canaries and official fixture classification remain
  later rows.
- The fresh status snapshot intentionally lives under `target/`; this proof
  records the durable summary so agents do not need target artifacts after
  cleanup.
