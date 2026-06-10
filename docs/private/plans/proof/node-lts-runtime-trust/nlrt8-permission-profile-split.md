# NLRT8 Permission Profile Split

Date: 2026-05-28
Agent: Codex

## Git Status Summary

- Baseline before this row: NLRT7 commit `814a06cb`
  (`Harden Node compatibility harness diagnostics`).
- Working tree also contained the pre-existing unrelated
  `docs/plans/dynamodb-adapter-plan.md` dirty file; NLRT8 does not depend on
  it and it remains unstaged.

## Files Changed

- `crates/nimbus-runtime/src/limits/grants.rs`
- `crates/nimbus-runtime/src/limits/resources.rs`
- `crates/nimbus-runtime/src/limits/tests.rs`
- `crates/nimbus-runtime/src/runtime_capabilities.rs`
- `crates/nimbus-runtime/src/runtime/tests/basic_invocation/node_capabilities.rs`
- `crates/nimbus-runtime/src/runtime/tests/basic_invocation/support.rs`
- `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`
- `crates/nimbus-tenant/src/runtime_admission.rs`
- `crates/nimbus-tenant/src/tests.rs`
- `crates/nimbus-tenant/src/operator_policy.rs`
- `crates/nimbus-tenant/src/operator_policy/evaluation.rs`
- `crates/nimbus-tenant/src/operator_policy/validation.rs`
- `crates/nimbus-tenant/src/operator_policy/tests.rs`
- `crates/nimbus-convex/src/registry/resolution/runtime_access.rs`
- `docs/architecture/runtime/permission-model.md`
- `docs/plans/node-lts-runtime-trust-plan.md`
- `docs/plans/proof/node-lts-runtime-trust/README.md`

## Decisions

- Made the ordinary `RuntimeLimits::application_node*()` and
  `RuntimeGrants::application_node()` constructors mean production in-process
  Node. They now carry generated-bundle read/write roots and narrow system
  metadata only.
- Added explicit local-development constructors:
  `RuntimeLimits::application_node*_local_development()` and
  `RuntimeGrants::application_node_local_development()`. These own loopback,
  listen, inspector, worker, and `NODE_TLS_REJECT_UNAUTHORIZED` compatibility
  grants.
- Added explicit service/microVM constructors:
  `RuntimeLimits::application_node*_service_microvm()` and
  `RuntimeGrants::application_node_service_microvm()`. These keep the broad
  Node service grant shape but are intended to be paired with the
  `micro_vm_service` tier.
- Kept compatibility targets as API shape only. The Node target no longer
  implies local host authority through constructor defaults.
- Updated the node-compat harness and package canary helper to request
  local-development limits explicitly, because those are compatibility
  measurement surfaces rather than production in-process tenant profiles.
- Updated operator policy lowering so `runtime.profile: nodeNN` chooses:
  production in-process grants for production `in_process_untrusted`,
  local-development grants when the policy mode is local development, and
  service/microVM grants when the tier is `micro_vm_service`.
- Added a tenant admission backstop that rejects custom production Node
  in-process policies that reintroduce `NODE_TLS_REJECT_UNAUTHORIZED`.

## Alternatives Rejected

- Rejected leaving `application_node*()` broad and adding only new production
  aliases. That would preserve the confusing default that caused the trust gap.
- Rejected relying solely on `nimbus-tenant` production admission. Admission is
  still fail-closed, but the lower-level constructors now communicate the safe
  production shape directly.
- Rejected silently weakening Node compatibility tests. Harnesses and local-dev
  tests were moved to explicit local-development profiles instead.

## Verification

- `cargo test -p nimbus-runtime node_permission_profiles -- --nocapture`: 1
  passed.
- `cargo test -p nimbus-runtime application_preset_supports_node_lts_targets
  -- --nocapture`: 1 passed.
- `cargo test -p nimbus-runtime node_capabilities -- --nocapture`: 7 passed.
- `cargo test -p nimbus-runtime network_permissions -- --nocapture`: 1 passed.
- `cargo test -p nimbus-runtime
  application_node22_local_development_permissions_allow_local_network_hosts
  -- --nocapture`: 1 passed.
- `cargo test -p nimbus-runtime package_resolution -- --nocapture`: 6 passed,
  3 ignored package canary watchpoints.
- `cargo test -p nimbus-runtime node_bootstrap -- --nocapture`: 9 passed.
- `cargo test -p nimbus-runtime node_compat_harness -- --nocapture`: 3 passed.
- `cargo test -p nimbus-tenant production_untrusted_runtime_admission
  -- --nocapture`: 8 passed.
- `cargo test -p nimbus-tenant node_profile -- --nocapture`: 4 passed.
- `cargo test -p nimbus-bridge runtime_execution_admission -- --nocapture`: 2
  passed.
- `cargo test -p nimbus-convex runtime_access -- --nocapture`: 2 passed.
- `cargo fmt --all --check`: pass after formatting.
- `npm run docs:validate-refs:strict`: pass.
- `git diff --check`: pass.

## Remaining Risks

- `application_web_standard()` still exposes the historical
  `NODE_TLS_REJECT_UNAUTHORIZED` env read. NLRT8 closed the Node production
  profile gap and added a Node-specific tenant admission backstop; a future
  non-Node policy sweep can decide whether web-standard local-dev and
  production env profiles should also split.
- Service/microVM constructors describe the intended runtime grant profile, but
  a production caller still needs an actual configured service/microVM backend
  before `nimbus-bridge` can execute that fallback instead of failing closed.
