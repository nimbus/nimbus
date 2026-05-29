# NFRC3 Node26 Current Target

Date: 2026-05-28
Authoring agent: Codex
Repository baseline: `e7e8b9d6`

## Git Status Summary

The worktree contains the active NFRC0-NFRC3 Node FaaS compatibility changes
and one unrelated pre-existing edit to `docs/plans/dynamodb-adapter-plan.md`.
The unrelated DynamoDB plan edit was not touched for this row.

## Relevant Node Tags

| Lane | Role | Upstream tag | Tag object | Commit |
| --- | --- | --- | --- | --- |
| Node20 | Legacy grace | `v20.20.2` | `35e07843146797923006aa01c6daabf4f53a4fb9` | `3626fea570e44896ad99aaf3bf6e59def5adede5` |
| Node22 | Maintenance LTS | `v22.22.3` | `354ef4b9bd94d5b662a9c300ddacc67f95a1bbe8` | `fdfa0ff0dbaf0fbf4d7d6d89a2ab807f3177fa5c` |
| Node24 | Active LTS | `v24.16.0` | `75143a8d75629c5d429dd0becb0d725e955f48fb` | `c7d10158bc31036de6783d66beaaaf551e3167aa` |
| Node26 | Current/non-LTS | `v26.2.0` | `30ffe3cfc2fda3684c38ec43aa79c381d398bf14` | `cfd7920d5a2d84905c4292362d01d07870047e93` |

## Files Changed

- `crates/nimbus-runtime/src/limits/axes.rs`
- `crates/nimbus-runtime/src/limits/resources.rs`
- `crates/nimbus-runtime/src/limits/tests.rs`
- `crates/nimbus-runtime/src/module_loader.rs`
- `crates/nimbus-runtime/src/runtime/bootstrap/transpile.rs`
- `crates/nimbus-runtime/src/runtime/driver/construction.rs`
- `crates/nimbus-runtime/src/runtime/tests/basic_invocation/node_bootstrap.rs`
- `crates/nimbus-tenant/src/operator_policy.rs`
- `crates/nimbus-tenant/src/operator_policy/tests.rs`
- `crates/nimbus-convex/src/lib.rs`
- `crates/nimbus-convex/src/registry/loading.rs`
- `crates/nimbus-convex/src/registry/resolution/runtime_access.rs`
- `docs/architecture/runtime/engine-seam.md`
- `docs/architecture/runtime/node-compat-surface-matrix.md`
- `docs/architecture/runtime/node-lts-compat/node-lts-lanes.json`
- `docs/architecture/runtime/node-lts-compat/node-lts-lanes.md`
- `docs/architecture/runtime/permission-model.md`
- `docs/plans/README.md`
- `docs/runtimes/nodejs/README.md`
- `docs/runtimes/nodejs/compatibility.md`
- `docs/runtimes/nodejs/configuration.md`
- `scripts/runtime/node/lane_registry.py`
- `scripts/runtime/node/publish_docs.py`
- `scripts/verify-node-lts-runtime-trust.sh`
- `tests/runtime/node/README.md`
- `tests/runtime/node/schemas/node-lts-lanes.schema.json`

## Decisions

- Promoted Node26 from registry-only metadata to a real
  `RuntimeCompatibilityTarget::Node26` on the existing `v8_deno_core` engine.
- Kept `node22` as the product default for this row. The product-default move
  to Node24 remains NFRC6.
- Kept `supported_node_lts_targets()` limited to Node22 and Node24. Node26 is
  selectable Current/non-LTS but not enterprise LTS support.
- Reused the existing Node-family bootstrap, module loader, extension
  transpiler, local-dev, service/microVM, and tooling grant constructors rather
  than creating Node26-specific behavior.
- Added Convex and tenant selectors for explicit Node26 policies while keeping
  default selection registry-driven.
- Documented Node26 as Current/non-LTS in public docs and architecture docs,
  without claiming that Nimbus embeds the official Node26 binary or `libnode`.

## Failure Inventory And Fixes

NFRC3 does not vendor or run the official Node26 fixture corpus; that broad
suite inventory is intentionally NFRC4/NFRC5. The broadest practical NFRC3
evidence was workspace compilation plus focused runtime/tenant/Convex selectors.

- Initial `cargo test -p nimbus-runtime node_lts -- --nocapture` failed at
  compile time because `module_loader.rs` and `runtime/driver/construction.rs`
  had exhaustive matches for Node20/Node22/Node24 but not Node26. Fixed by
  routing Node26 through the same Node-family module-loader and snapshot paths.
- Initial `cargo test -p nimbus-runtime node26_current_target_exposes_truthful_process_metadata -- --nocapture`
  failed at runtime with a Deno-core inspector unwrap panic. The root cause was
  that runtime options enabled `inspector` for Node20/Node22/Node24 but not
  Node26 while bootstrap state installs an inspector for every Node target.
  Fixed by enabling the inspector option for Node26.

## Verification

- `cargo test -p nimbus-runtime node_lts -- --nocapture`: pass, 3 tests passed,
  0 failed, 518 filtered out.
- `cargo test -p nimbus-runtime node26_current_target_exposes_truthful_process_metadata -- --nocapture`:
  pass, 1 test passed, 0 failed, 520 filtered out.
- `cargo test -p nimbus-runtime tooling_preset_requires_node_target -- --nocapture`:
  pass, 1 test passed, 0 failed, 520 filtered out. The printed panic is the
  expected caught panic for a non-Node tooling target.
- `cargo test -p nimbus-tenant node_runtime_profiles_follow_lts_registry_targets -- --nocapture`:
  pass, 1 test passed, 0 failed, 78 filtered out.
- `cargo test -p nimbus-convex convex_node_runtime_lanes_follow_lts_registry_targets -- --nocapture`:
  pass, 1 test passed, 0 failed, 9 filtered out.
- `cargo test -p nimbus-convex convex_use_node_action_package_canary_node26_current -- --ignored --nocapture`:
  pass, 1 ignored canary executed and passed.
- `bash scripts/verify-node-lts-lanes.sh`: pass, 4 lanes, product default
  `node22`, consumers `nimbus-runtime`, `nimbus-tenant`, `nimbus-bridge`,
  `nimbus-convex`.
- `bash scripts/verify-node-latest-suite-tags.sh`: pass, 4 lanes, 3 needing
  fixture sync; negative self-tests passed.
- `npm run docs:validate-refs:strict`: pass, 225 working-tree Markdown files.
- `bash scripts/verify-node-lts-docs.sh`: pass; evidence docs current and docs
  guard passed.
- `cargo check --workspace`: pass, finished in 14.20s.
- `git diff --check`: pass.

## Remaining Risks

- Node26 has no vendored official fixture corpus yet. NFRC4 owns syncing or
  deliberately comparing the latest Node26 corpus and producing the first wide
  suite issue inventory.
- Node26 official fixture classification and app canary evidence are not yet
  complete. NFRC5, NFRC7, and NFRC8 own those gates before docs can claim
  Node26 Current-line app support.
- Product default remains Node22 until NFRC6 moves it to Node24 across runtime,
  tenant, Convex, docs, and diagnostics.
