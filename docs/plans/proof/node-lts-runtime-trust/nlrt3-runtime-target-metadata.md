# NLRT3 Runtime Target Metadata

Date: 2026-05-28
Authoring agent: Codex
Status: done

## Scope

Wire runtime compatibility target metadata to the checked-in Node LTS lane
registry. Keep the public enum stable, keep config parsing for `20`, `22`, and
`24`, make Node20 EOL status test-visible, and prove the extracted
`nimbus-tenant` and `nimbus-convex` owners stay synchronized with the registry.

## Files Changed

- `crates/nimbus-runtime/src/limits/axes.rs`
- `crates/nimbus-runtime/src/limits/tests.rs`
- `crates/nimbus-runtime/src/limits.rs`
- `crates/nimbus-runtime/src/lib.rs`
- `crates/nimbus-tenant/src/operator_policy.rs`
- `crates/nimbus-tenant/src/operator_policy/tests.rs`
- `crates/nimbus-convex/src/manifest.rs`
- `crates/nimbus-convex/src/registry/resolution/runtime_access.rs`
- `docs/plans/node-lts-runtime-trust-plan.md`
- `docs/plans/proof/node-lts-runtime-trust/README.md`
- `docs/plans/proof/node-lts-runtime-trust/nlrt3-runtime-target-metadata.md`

## Decisions

- Kept `RuntimeCompatibilityTarget` as the public selector enum, but made its
  Node metadata lookup registry-backed through
  `docs/architecture/runtime/node-lts-compat/node-lts-lanes.json`.
- Added `RuntimeNodeSupportPhase` and `RuntimeNodeLtsLane` as public diagnostic
  metadata so consumers can reason about EOL, Maintenance LTS, Active LTS, and
  preview-current status without duplicating the JSON parser.
- Removed the hard-coded synthetic `v20.0.0-nimbus`, `v22.0.0-nimbus`, and
  `v24.0.0-nimbus` values from `axes.rs`. `node_runtime_version()` now returns
  the registry `upstream_tag`, and `node_runtime_version_number()` returns the
  registry `upstream_version`.
- Kept Node26 unparseable as a runtime target. It remains present in the
  registry as preview-current but has no `runtime_compatibility_target` until a
  later promotion changes both registry data and tests.
- Changed Convex's default `"use node"` selection to use
  `RuntimeCompatibilityTarget::product_default_node_lts_target()` instead of a
  literal `Node22`.
- Changed tenant operator Node profile mapping to flow through
  `RuntimeCompatibilityTarget` and added numeric aliases such as `profile:
  "22"` while preserving existing `node22` input.

## Registry-Backed Contract

Runtime now exposes:

- `RuntimeCompatibilityTarget::product_default_node_lts_target()`
- `RuntimeCompatibilityTarget::configured_node_lts_targets()`
- `RuntimeCompatibilityTarget::supported_node_lts_targets()`
- `RuntimeCompatibilityTarget::node_lts_metadata()`
- `RuntimeCompatibilityTarget::node_support_phase()`
- `RuntimeCompatibilityTarget::is_supported_node_lts()`
- registry-derived `node_major_version()`, `node_runtime_version()`,
  `node_runtime_version_number()`, and `node_release_lts_codename()`

The tests assert:

- `20`, `node20`, and `Node20` parse as `Node20`;
- `22`, `node22`, and `Node22` parse as `Node22`;
- `24`, `node24`, and `Node24` parse as `Node24`;
- `26` fails because Node26 is preview-only without a runtime target;
- product default is `Node22`;
- active supported LTS targets are exactly `Node22` and `Node24`;
- `Node20` remains configured for legacy-grace evidence but is not supported
  active enterprise LTS.

## Verification

```text
cargo test -p nimbus-runtime node_lts -- --nocapture
3 passed; 0 failed; 0 ignored; 510 filtered out
```

```text
cargo test -p nimbus-tenant node_runtime_profiles_follow_lts_registry_targets -- --nocapture
1 passed; 0 failed; 0 ignored; 74 filtered out
```

```text
cargo test -p nimbus-convex convex_node_runtime_lanes_follow_lts_registry_targets -- --nocapture
1 passed; 0 failed; 0 ignored; 6 filtered out
```

```text
rg -n "v20\.0\.0-nimbus|v22\.0\.0-nimbus|v24\.0\.0-nimbus" crates/nimbus-runtime/src/limits/axes.rs
no matches
```

```text
cargo fmt --all --check
pass
```

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

- NLRT4 still needs to make JavaScript-visible `process.version`,
  `process.versions.node`, `process.release.lts`, and ABI/module metadata
  truthful per lane. Today `source.rs` still synthesizes process globals from
  the compatibility target string.
- NLRT5 still needs generated evidence and public docs to stop centering older
  `lane_role`/`public_contract_role` language around Node22.
