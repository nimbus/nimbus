# Node Compatibility Supported Lanes Plan

Status: `done`

Owner: Node-compatible runtime / test infrastructure

This plan owns the post-matrix-classification wave after the completed Node22
path-owned evidence correction. It promotes Node20, Node22, and Node24 from
internal lane roles into explicit supported compatibility targets while keeping
Node22 as the default runtime target until Node24 becomes the default.

## Objective

Make Node20, Node22, and Node24 first-class user-selectable compatibility lanes
with the same evidence standard:

- path-owned green fixture inventory
- explicit red/gap classifications for known failing behavior
- explicit skip/exclusion classifications for non-runnable or non-contract
  files
- zero unclassified vendored tests in generated status/dashboard outputs
- clear user-facing defaults where Node22 remains default until the Node24
  migration is intentionally completed

## Guardrails

- Do not weaken tests, delete assertions, or convert failures into green claims.
- Do not call a skipped support fixture "red"; distinguish known red/gap from
  skipped/excluded.
- Do not keep Node20 and Node24 as second-class legacy lane roles
  in public outputs once this plan lands.
- Keep Node22 as the default lane until a separate default-migration gate says
  Node24 is ready.
- Keep support claims tied to generated status, inventory, dashboard, trend,
  and published evidence artifacts.

## Starting Baseline

- Pre-work Node20 reported `913 / 1308` documented green and `395`
  unclassified tests while still using an internal secondary-lane role.
- Pre-work Node22 reported `898` path-owned green plus `385` classified files
  for full `1283 / 1283` coverage, and remains the default compatibility
  target.
- Pre-work Node24 reported `925 / 1495` documented green and `570`
  unclassified tests while still using an internal forward-lane role.
- Dashboard/reporting internals used a vague classified-remainder field that
  blurred expected skips, exclusions, known red failures, and owner-backed gaps.
- Current generated evidence reports Node20 `917 / 1308` green plus `391`
  classified red/skip entries, Node22 `876 / 1283` green plus `407`
  classified red/skip entries, and Node24 `926 / 1495` green plus `569`
  classified red/skip entries, with zero unclassified vendored tests in all
  three lanes.

## Checkpoints

| ID | Status | Exit criteria |
| --- | --- | --- |
| NCL1 Lane model and user contract | `done` | Lane metadata, docs, and generated reports model Node20, Node22, and Node24 as supported lanes, with Node22 marked as the current default and Node24 marked as supported but not default. |
| NCL2 Clear red/skip terminology | `done` | Status, dashboard, inventory, schemas, docs, and code expose clear categories: green, known red/gap, skipped/excluded, and unclassified. Legacy vague remainder wording was migrated to red/skip terminology. |
| NCL3 Node20 path-owned classification | `done` | Node20 uses path-owned green evidence and a lane classification catalog so generated status reports zero unclassified vendored tests without overclaiming support. |
| NCL4 Node24 path-owned classification | `done` | Node24 uses path-owned green evidence and a lane classification catalog so generated status reports zero unclassified vendored tests without overclaiming support. |
| NCL5 User workflow support | `done` | Make targets, docs, and runtime/test tooling make it obvious how users select `node20`, `node22`, or `node24`, with Node22 as the default lane until a Node24-default plan is completed. |
| NCL6 Closeout audit | `done` | Status, inventory, dashboard, trends, published evidence, schemas, docs, and focused runtime tests agree for all three lanes; plan is moved to stable baselines only after reproducible verification. |

## Verification Gates

- `make node-compat-status`
- `make node-compat-inventory LANE=node20`
- `make node-compat-inventory LANE=node22`
- `make node-compat-inventory LANE=node24`
- `make node-compat-dashboard`
- `make node-compat-trends`
- `make node-compat-publish-evidence`
- schema validation for generated status/inventory/trend/refresh artifacts
- focused `cargo test -p neovex-runtime node_compat_manifest_topology -- --nocapture --test-threads=1`
- focused runtime lane tests for any promoted green batch
- `cargo fmt --all --check`
- `git diff --check`

## Progress Log

- `2026-05-12`: Started after completing
  `docs/plans/archive/node-compat-matrix-classification-plan.md`. First slice is to
  replace internal secondary-lane language with first-class supported-lane and
  clear red/skip terminology before extending the Node22 path-owned
  classification model to Node20 and Node24.
- `2026-05-12`: Promoted lane metadata to Node20 supported, Node22 default,
  and Node24 supported; added lane classification catalogs for Node20 and
  Node24; migrated generated status/inventory/dashboard fields to
  `classified_red_skip_count`; refreshed public dashboard/trend/evidence; and
  reran application/tooling canaries outside the sandbox for host-required
  localhost/IPC evidence.
- `2026-05-12`: Closed the plan after reproducible verification:
  expectation validation, classification check mode, status/inventory for all
  three lanes, dashboard/trends/published evidence, claim validation, focused
  manifest tests, `cargo fmt --all --check`, and `git diff --check`.
