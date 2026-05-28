# NLRT4 Truthful Process Metadata

Date: 2026-05-28

Agent: Codex

## Git Status Summary

NLRT4 changes are present in the working tree but not yet committed at proof
write time. The unrelated pre-existing dirty file remains
`docs/plans/dynamodb-adapter-plan.md` and is intentionally excluded from this
slice.

## Decisions

- Added Node release and ABI/module metadata to the checked-in LTS lane
  registry. The registry now cites Node's official ABI version registry in
  addition to the release schedule, release, EOL, and download sources.
- Exposed a `node_api_contract` descriptor through the runtime bootstrap
  contract instead of reconstructing Node process metadata from the
  compatibility-target enum string.
- Installed `process.version`, `process.versions.node`,
  `process.versions.modules`, and `process.release` from lane registry data.
  `process.version` and `process.versions.node` now use exact upstream tags and
  version numbers for the selected lane.
- Preserved the existing Deno/Node substrate process object by cloning its
  descriptors into a Nimbus-owned process wrapper. This lets Nimbus own
  lane-contract fields without trying to redefine Deno's non-configurable
  `process.release` property.
- Made the node-compat fixture executor lane-aware. Seeded Node20 and Node24
  fixture runs now use `RuntimeLimits::application_node20()` and
  `RuntimeLimits::application_node24()` instead of silently running on the
  Node22 default.
- Reclassified `process-and-timing-supplementary` from expected failure to
  sequential evidence because `supplementary-process-release-shape` is now
  green across the carried lanes.

## Changed Files

- `docs/architecture/runtime/node-lts-compat/node-lts-lanes.json`
- `docs/architecture/runtime/node-lts-compat/node-lts-lanes.md`
- `tests/runtime/node/schemas/node-lts-lanes.schema.json`
- `scripts/runtime/node/lane_registry.py`
- `crates/nimbus-runtime/src/limits/axes.rs`
- `crates/nimbus-runtime/src/limits/tests.rs`
- `crates/nimbus-runtime/src/runtime/bootstrap/ops/shared.rs`
- `crates/nimbus-runtime/src/runtime/bootstrap/source.rs`
- `crates/nimbus-runtime/src/runtime/tests/basic_invocation/node_bootstrap.rs`
- `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`
- `crates/nimbus-runtime/src/runtime/tests/node/cases/watchpoints_extended.rs`
- `crates/nimbus-runtime/src/runtime/tests/node/manifest_topology.rs`
- `crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/supplementary/process-release-shape.node20.js`
- `crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/supplementary/process-release-shape.node22.js`
- `crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/supplementary/process-release-shape.node24.js`
- `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/fixtures/process-and-timing-supplementary.json`
- `docs/architecture/runtime/node-compat-supplementary.md`
- `docs/architecture/runtime/node-compat-supplementary-failures.md`

## Verification

- `bash scripts/verify-node-lts-lanes.sh`: pass; validated 4 lanes, product
  default `node22`, and consumers `nimbus-runtime`, `nimbus-tenant`,
  `nimbus-bridge`, and `nimbus-convex`.
- `cargo test -p nimbus-runtime node_lts -- --nocapture`: 3 passed, 0 failed,
  0 ignored.
- `cargo test -p nimbus-runtime node22_target_exposes_minimal_node_globals -- --nocapture`:
  1 passed, 0 failed, 0 ignored.
- `cargo test -p nimbus-runtime node22_target_delivers_manual_process_warning_events -- --nocapture`:
  1 passed, 0 failed, 0 ignored.
- `cargo test -p nimbus-runtime node_compat_supplementary_process_shape -- --nocapture --test-threads=1`:
  3 passed, 0 failed, 0 ignored.
- `cargo test -p nimbus-runtime node_compat_runtime_limits_only_grant_self_exec_to_known_respawn_fixtures -- --nocapture`:
  1 passed, 0 failed, 0 ignored.
- `cargo test -p nimbus-runtime manifest_topology -- --nocapture`: 17 passed,
  0 failed, 0 ignored.
- `cargo fmt --all --check`: pass.
- `npm run docs:validate-refs:strict`: pass, 219 working-tree Markdown files.
- `git diff --check`: pass.

## Acceptance Evidence

- Per-lane supplementary fixtures now assert exact `process.version`,
  `process.versions.node`, `process.release.name`, `process.release.lts`, and
  `process.versions.modules` values.
- Node22 minimal bootstrap coverage asserts the exact default-lane metadata:
  `v22.22.3`, `22.22.3`, release name `node`, LTS codename `Jod`, and module
  ABI `127`.
- Supported LTS lanes Node22 and Node24 pass the supplementary
  process-release-shape probes. Node20 also passes as legacy-grace regression
  coverage, but remains `eol_legacy` rather than active enterprise LTS.
- The supplementary failure inventory no longer lists
  `supplementary-process-release-shape` as an active measured failure.
- Embedded component versions outside the Node API contract remain documented
  as Nimbus substrate diagnostics, not as a claim of full native Node patch
  parity.

## Follow-On

Generated dashboard evidence still belongs to the later NLRT evidence/doc
refresh work. NLRT4 closed the runtime behavior, manifest classification, and
hand-written failure inventory for the process-release-shape slice.
