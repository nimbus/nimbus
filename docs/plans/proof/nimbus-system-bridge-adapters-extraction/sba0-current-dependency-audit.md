# SBA0 Current Dependency Audit

Date: 2026-05-27
Status: completed
Scope: proposed `nimbus-system`, `nimbus-bridge`, `nimbus-auth`,
`nimbus-adapters`, and follow-on extraction plan.

## Commands Run

```bash
cargo metadata --no-deps --format-version 1
cargo tree -p nimbus-server --edges normal --depth 1
cargo tree -p nimbus-server --edges normal --invert nimbus-server --workspace --depth 2
rg --count-matches "crate::(runtime_host|system_tenant|tenant|local_enforcement|execution|application_auth|service_manager|service_registry|sandbox|state|router|http|local_server|artifact_verifier_effects|license|machines|scheduling)" crates/nimbus-server/src/adapters -g '*.rs'
rg -n "crate::(adapters|runtime_host|system_tenant|tenant|local_enforcement|execution|application_auth|service_manager|service_registry|sandbox|state|router|http|local_server|artifact_verifier_effects|license|machines|scheduling)" crates/nimbus-server/src/system_tenant -g '*.rs'
rg -n "crate::(adapters|runtime_host|system_tenant|tenant|local_enforcement|execution|application_auth|service_manager|service_registry|sandbox|state|router|http|local_server|artifact_verifier_effects|license|machines|scheduling)" crates/nimbus-server/src/runtime_host crates/nimbus-server/src/execution/host_state.rs crates/nimbus-server/src/execution/read_tracking crates/nimbus-server/src/execution/runtime_admission.rs crates/nimbus-server/src/execution/errors.rs -g '*.rs'
find crates/nimbus-server/src -name '*.rs' -print0 | xargs -0 wc -l | sort -nr | head -40
find crates/nimbus-server/src/adapters -name '*.rs' -print0 | xargs -0 wc -l | sort -nr | head -35
find crates/nimbus-server/src/system_tenant crates/nimbus-server/src/runtime_host crates/nimbus-server/src/artifact_verifier_effects -name '*.rs' -print0 | xargs -0 wc -l | sort -nr
```

## Cargo Graph Findings

`nimbus-server` currently has normal workspace dependencies on:

- `nimbus-core`
- `nimbus-engine`
- `nimbus-machine`
- `nimbus-node`
- `nimbus-runtime`
- `nimbus-sandbox`
- `nimbus-tenant`

Only `nimbus` and `nimbus-bin` depend on `nimbus-server` as normal workspace
dependencies.

Conclusion: new crates can sit below `nimbus-server` without a workspace cycle
if they never depend on `nimbus-server`.

## Size Findings

`crates/nimbus-server/src` currently totals about 88k lines.

High-pressure ownership areas:

- `crates/nimbus-server/src/adapters`: about 40k lines.
- `crates/nimbus-server/src/system_tenant`: about 2.4k lines.
- `crates/nimbus-server/src/runtime_host`: about 900 lines.
- `crates/nimbus-server/src/artifact_verifier_effects`: about 2.5k lines.

Largest server files are mostly adapter protocol handlers and tests, with
notable non-adapter roots:

- `tenant_isolation_drift.rs`: 1173 lines.
- `service_manager.rs`: 1136 lines.
- `system_tenant/records.rs`: 1049 lines.
- `artifact_verifier_effects.rs`: 860 lines.
- `router.rs`: 784 lines.

Conclusion: line count supports the extraction pressure, but the plan should
remain boundary-driven because the highest-trust modules are not always the
largest modules.

## Owner Classification Table

| Current area | Planned owner | Notes |
| --- | --- | --- |
| `router`, `construction`, `state`, listener setup, top-level HTTP/WS mounting | `nimbus-server` | Server remains the composition root and transport owner. |
| `system_tenant/*` | `nimbus-system` | Blocked by Convex deployment summary input until neutral record inputs exist. |
| `runtime_host/*` | `nimbus-bridge` | Provider-neutral runtime capability code; bridge-adjacent execution helpers need classification first. |
| `execution/host_state.rs`, `execution/read_tracking/*`, `execution/errors.rs`, `execution/runtime_admission.rs` | `nimbus-bridge` or `nimbus-server` by per-file proof | Read tracking and admission look bridge-owned; worker/invocation execution needs separate review. |
| Neutral parts of `application_auth.rs` | `nimbus-auth` | Verifier trait, resolved auth, principal normalization, neutral bearer parsing, and classified auth errors. |
| Deployment/transport parts of `application_auth.rs` | `nimbus-server` | `AppState`, `DeploymentState`, axum headers, tonic metadata, Firebase emulator toggles, and router wiring stay out of `nimbus-auth`. |
| `adapters/convex`, `adapters/firebase`, `adapters/cloud_functions`, `adapters/mongodb` | `nimbus-adapters` | Extract only after system, bridge, and auth public seams exist. |
| `provider_family/firestore.rs` | `nimbus-adapters` unless proven core | It is Firestore-family compatibility code today. |
| `artifact_verifier_effects/*` | `nimbus-artifacts` then `nimbus-provenance` split as earned | Process-backed verifier effects must not leak into pure provenance models. |
| `execution/invocations::RuntimeBundleProvenanceConfig` | `nimbus-provenance` or `nimbus-bridge` by proof | Runtime provenance gating currently crosses execution and artifact verification. |
| `local_server/*`, `http/local_admin.rs`, operator console access, deploy admin admission | `nimbus-operator` if earned | Route registration and `AppState` stay server-owned. Local admin token remains a credential name, not the crate name. |
| `service_manager.rs`, `service_registry.rs`, `sandbox.rs` server wrappers | `nimbus-services` if earned | Extract only if service lifecycle composition can avoid router/listener ownership. |
| `license/*` | `nimbus-license` if shared outside server metadata | Currently the cleanest follow-on candidate but still needs reuse proof. |
| server integration tests | Usually `nimbus-server` | Tests that prove cross-crate wiring remain with server. |
| adapter protocol unit tests | `nimbus-adapters` | Move with adapter modules when extraction happens. |

## `nimbus-system` Blockers

Current system tenant code is close to a real boundary, but not clean yet.

Observed imports:

- `system_tenant/records.rs` imports `crate::adapters::convex::ConvexRegistryDeploySummary`.
- `system_tenant/records.rs` imports `crate::local_enforcement::*`, which is
  currently only a server-local re-export of `nimbus-node`.
- `system_tenant/records.rs` imports `crate::tenant::TenantIsolationContext`.

Primary blocker:

- `record_convex_deployment_state_async` and `deployment_bundle_sha256` accept
  `ConvexRegistryDeploySummary`, which makes system evidence depend on an
  adapter-private deployment shape.

Required readiness move:

- Introduce neutral system deployment record inputs before extracting
  `nimbus-system`.

## `nimbus-bridge` Blockers

`runtime_host/*` is already provider-neutral in intent, matching
`docs/architecture/runtime/adapter-boundary.md`, but it is still server-shaped
in imports.

Observed imports:

- `runtime_host/mod.rs` imports `crate::execution::host_state::RuntimeHostState`.
- `runtime_host/mod.rs` imports `crate::local_enforcement::LocalEnforcementBinding`.
- `runtime_host/mod.rs` imports `crate::tenant::{TenantIsolationDecision, TenantStorageAccessDecision}`.
- `runtime_host/capabilities.rs` imports `crate::execution::errors::*`.
- `runtime_host/abi/document_calls.rs` imports `crate::runtime_host::*`.
- `execution/runtime_admission.rs` imports tenant authority types.
- `execution/host_state.rs` imports runtime read tracking.

Primary blockers:

- Bridge-adjacent execution helpers have not been classified into bridge-owned,
  runtime-owned, adapter-owned, or server-owned.
- `local_enforcement.rs` is only `pub use nimbus_node::*`, so extracted bridge
  code should use `nimbus-node` directly or an intentional compatibility
  re-export.

Required readiness move:

- Split generic runtime bridge helpers from adapter-specific invocation and
  host-call dispatch before moving files.

## `nimbus-auth` Extraction Decision

Decision: extract `nimbus-auth` after `nimbus-bridge` and before
`nimbus-adapters`.

Reasoning:

- Application auth is consumed by multiple adapter families and server
  composition.
- Existing architecture docs say adapters should consume shared auth rather
  than owning principal normalization or bearer verification semantics.
- Leaving shared auth in `nimbus-server` would force extracted adapters to keep
  a server-private dependency or duplicate auth normalization.
- The boundary is enterprise-trust-relevant because it separates tenant/user
  application auth from local admin/operator authority.

Application auth is not ready to move as-is, because the current module mixes
neutral auth contracts with server deployment state and transport parsing.

Observed dependencies in `application_auth.rs`:

- `AppState`
- `DeploymentState`
- axum headers
- tonic metadata
- `nimbus_runtime::InvocationAuth`

Observed adapter imports:

- Convex, Firebase, and Cloud Functions adapter code import
  `crate::application_auth` for principal normalization, bearer verification,
  auth error mapping, and request handling.

Extraction target:

- `ApplicationAuthVerifier`
- `ResolvedApplicationAuth`
- principal normalization
- neutral bearer-value parsing
- subject alias normalization
- classified auth errors that can be mapped by server/adapters

Keep out of `nimbus-auth`:

- `AppState`
- `DeploymentState`
- axum header ownership
- tonic metadata ownership
- router wiring
- local admin token authority
- adapter registries

Conclusion:

- `nimbus-auth` is now a decided extraction in the plan.
- The implementation must split neutral auth contracts from server
  deployment/transport adapters before moving files.

## `nimbus-adapters` Blockers

Adapters are the largest extraction target, but currently import many
server-private modules.

Observed server-private imports include:

- `crate::state`
- `crate::application_auth`
- `crate::local_server`
- `crate::system_tenant`
- `crate::runtime_host`
- `crate::service_registry`
- `crate::execution`
- `crate::tenant`
- `crate::router`
- `crate::provider_family`

Primary blockers:

- Adapters need server state and deployment snapshots.
- Adapters write system evidence directly.
- Adapters call runtime host internals directly.
- Adapters rely on server-local auth and local-server audit helpers.
- Firestore-family shared helpers live outside adapters but are provider-family
  compatibility code.

Required readiness move:

- Introduce small composition traits for auth, system evidence, runtime bridge,
  service registry, sandbox catalog, and local audit before extracting
  adapters.

## Follow-On Crate Findings

`nimbus-artifacts` and `nimbus-provenance` are plausible but should remain
after `nimbus-adapters` in this plan.

Evidence:

- `artifact_verifier_effects/*` owns process-backed verifier runners.
- Artifact verifier effects depend on tenant artifact contracts.
- `execution/invocations` owns `RuntimeBundleProvenanceConfig`.
- Convex and Cloud Functions registries configure runtime bundle provenance.

Conclusion:

- `nimbus-artifacts` should first separate artifact contracts from process
  verifier effects.
- `nimbus-provenance` should own provenance/SBOM/SLSA evidence models only
  after runtime provenance gating is no longer embedded in server-private
  execution code.

`nimbus-operator` is the chosen name for the operator/admin follow-on crate:

- Local admin security currently crosses `local_server`, `http`, `state`, and
  system evidence.
- Extract only if admin security models can be separated from route
  registration and `AppState`.
- The repo already uses "operator console," "operator policy," and "operator
  workflows" for the human/control-plane role. "Admin" remains appropriate for
  credential and route names such as local admin token and deploy admin API, but
  the crate should name the role boundary rather than one credential type.

`nimbus-services` is plausible but not ready:

- `service_manager.rs` depends on sandbox catalog, tenant image verification,
  runtime service registry, system tenant setup, and router tests.
- Extract only if service lifecycle composition can consume system and tenant
  interfaces without owning HTTP routes.

`nimbus-license` is the cleanest follow-on candidate:

- `license/*` appears self-contained and does not import server-private modules.
- It should still extract only if the license surface is shared beyond server
  metadata routes.

## Plan Review Findings

The original plan was directionally correct but not yet strict enough for an
enterprise-trust coding-agent run.

Required plan additions now applied:

- Verified baseline section.
- Explicit dependency allow/deny matrix.
- Agent execution protocol for compaction-safe work.
- Decided `nimbus-auth` extraction gate.
- Provider-family helper classification.
- Direct `nimbus-node` import rule for the server-local `local_enforcement`
  shim.
- Per-crate success criteria for follow-on extraction candidates.
- Verifier checks for auth and follow-on decisions.

Residual risk:

- The source graph audit used text import search, not a compiler-level module
  dependency extractor. The verifier should keep text checks, but final
  extraction must be proven with `cargo check --workspace` and focused tests.
