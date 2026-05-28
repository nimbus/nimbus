# SSE0 Baseline Seam Audit Proof

Date: 2026-05-28
Status: completed

## Scope

Inventory retained `nimbus-server` seams after the completed system/bridge/auth
/ license extraction wave and add the verifier skeleton for
`docs/plans/server-seam-extraction-readiness-plan.md`.

## Task Checklist

- [x] SSE0.1 Inventory retained server seams.
- [x] SSE0.2 Record import graph.
- [x] SSE0.3 Add verifier skeleton.
- [x] SSE0.4 Establish denied-import patterns.
- [x] SSE0.5 Preserve previous extraction gate.

## Owner Table

| Retained seam | Current owner | Readiness posture |
| --- | --- | --- |
| MongoDB adapter | `crates/nimbus-server/src/adapters/mongodb` | Partly clean. Protocol, BSON bridge, commands, connection, and auth are mostly value/protocol owned. Listener lifecycle remains server-owned until SSE1A proves a narrow listener boundary. |
| Firebase/provider-family | `crates/nimbus-server/src/adapters/firebase` plus `crates/nimbus-server/src/provider_family` | Mixed. Firestore document/operation helpers may be adapter/data-model owned, but REST/gRPC/listen handlers still accept `AppState` and use server application-auth usage recording. |
| Cloud Functions adapter | `crates/nimbus-server/src/adapters/cloud_functions` | Mixed and effectful. HTTP protocol handling is adapter-shaped, but execution, runtime bundle provenance, generated bundle process tests, `RouterBuildConfig` test fixtures, and service registry inputs keep this in server readiness scope. |
| Convex adapter | `crates/nimbus-server/src/adapters/convex` | Largest mixed owner. Routes, handlers, host bridge, runtime-backed execution, scheduler, subscriptions, deploy summaries, and tests need sub-owner proof before any extraction. |
| Artifact verifier effects | `crates/nimbus-server/src/artifact_verifier_effects.rs` and `crates/nimbus-server/src/artifact_verifier_effects/*` | Effectful server/operator wiring. Pure policy/contracts are already tenant-owned; process-backed `cosign`, `slsa`, and SBOM runners remain intentionally server-owned until SSE2. |
| Provenance/runtime bundle admission | `crates/nimbus-server/src/execution/*`, Cloud Functions/Convex registries, and runtime bundle config consumers | Split by owner. Runtime manifest admission and tenant image provenance policy need classification before deciding whether `nimbus-provenance` is coherent. |
| Services/service registry/sandbox service traits | `crates/nimbus-server/src/service_manager*`, `service_registry.rs`, `sandbox.rs`, `local_enforcement.rs` | Still server-owned because service evidence writes and runtime service lookup are coupled to server composition, `nimbus-system`, and local enforcement shims. |
| Operator/local admin | `crates/nimbus-server/src/local_server/*`, `http/local_admin.rs`, `http/deploy.rs`, `http/ui.rs`, `router.rs` | Transport-coupled. Token/session/origin policy has extractable value logic, but Axum middleware, route mounting, shutdown/audit/system-event effects, and deploy admin remain server-owned. |
| Composition-only modules | `router.rs`, `state.rs`, `construction.rs`, HTTP/WS route mounting | Must remain in `nimbus-server`. These own `AppState`, `DeploymentState`, `RouterBuildConfig`, listener lifecycle, and route composition. |

## Import Graph Summary

Commands run:

```bash
rg -n "crate::(state|router|local_server|system_tenant|application_auth|runtime_host|service_registry|service_manager|sandbox|tenant|local_enforcement|artifact_verifier_effects|execution)" crates/nimbus-server/src/adapters -g '*.rs'
rg -n "AppState|DeploymentState|RouterBuildConfig|LocalServerAccessPolicy|record_system_event_async|record_service_handle_async|std::process::Command|ProcessArtifactVerifierCommandRunner" crates/nimbus-server/src -g '*.rs'
cargo tree -p nimbus-server --edges normal --depth 1
```

Important results:

- `cargo tree -p nimbus-server --edges normal --depth 1` shows
  `nimbus-server` depends on the previously extracted `nimbus-auth`,
  `nimbus-bridge`, `nimbus-license`, `nimbus-system`, `nimbus-tenant`,
  `nimbus-node`, `nimbus-runtime`, `nimbus-sandbox`, `nimbus-engine`, and
  `nimbus-core` crates. This confirms the next seams should move away from
  server, not back into it.
- No adapter imports `crate::runtime_host` or `runtime_host::`; this preserves
  the previous bridge extraction invariant.
- MongoDB adapter imports are narrow: current hits are tenant context imports
  in `commands/session.rs`, `commands/tenant.rs`, and test/support paths. No
  `AppState`, router, local-server, system-tenant, application-auth, runtime
  host, service registry, or process-effect hits were found in MongoDB adapter
  production files.
- Firebase adapter still imports `AppState` in REST/gRPC/listen paths:
  `operations.rs`, `mod.rs`, `grpc/mod.rs`, `grpc/unary.rs`,
  `grpc/listen_stream.rs`, and `grpc/write_stream.rs`. It also imports
  server-private `application_auth` helpers in REST/gRPC handlers.
- Cloud Functions imports server state and execution/service composition:
  `http.rs`, `http/invocation.rs`, `http/callable.rs`, `http/tenant.rs`,
  `execution.rs`, `host_bridge.rs`, and `registry.rs` hit `AppState`,
  `DeploymentState`, `RouterBuildConfig`, `crate::execution`,
  `RuntimeServiceRegistry`, and tenant admission types.
- Convex imports the widest set of server seams: local-server authorization and
  route families, `AppState`, `system_tenant` scheduler/deployment/run/
  subscription evidence, `service_registry`, runtime execution/subscription
  helpers, and tenant admission types across handlers, host bridge,
  subscriptions, runtime-backed execution, scheduling, and tests.
- Artifact verifier effects still construct default process runners through
  `ProcessArtifactVerifierCommandRunner` in `artifact_verifier_effects.rs`,
  `artifact_verifier_effects/cosign.rs`, `artifact_verifier_effects/slsa.rs`,
  and `artifact_verifier_effects/sbom.rs`.
- `std::process::Command` appears in server-owned system/install-method code
  and Cloud Functions test helpers, plus artifact verifier runner code. These
  are denied for pure artifact/provenance candidates and must be classified in
  SSE2/SSE3.
- Services still write observed system evidence through
  `record_service_handle_async` in `tenant_isolation_drift.rs` and
  `service_manager/system_state.rs`.
- Operator/local-admin/deploy surfaces still carry `AppState`,
  `DeploymentState`, `LocalServerAccessPolicy`, and `RouterBuildConfig` through
  `state.rs`, `router.rs`, `local_server/middleware.rs`, `http/local_admin.rs`,
  `http/deploy.rs`, `http/ui.rs`, and tests. This confirms `nimbus-operator`
  is not ready before SSE5.

## Denied Import Patterns

Baseline denied patterns by candidate:

| Candidate | Denied before extraction |
| --- | --- |
| MongoDB adapter | `AppState`, `DeploymentState`, `RouterBuildConfig`, `crate::router`, `crate::local_server`, `crate::system_tenant`, `crate::runtime_host`, `crate::application_auth`, listener lifecycle ownership. |
| Firebase/provider-family | `AppState`, `DeploymentState`, router/listener lifecycle, `crate::application_auth` private helpers, direct `crate::system_tenant` writes, runtime host internals. |
| Cloud Functions adapter | `AppState`, `DeploymentState`, `RouterBuildConfig`, `crate::execution` runtime/provenance internals unless trait-inverted, private runtime host internals, direct process/provenance effects, direct `_nimbus` writes. |
| Convex adapter | `AppState`, router/listener lifecycle, `crate::local_server` auth/audit policy in pure handlers, `crate::system_tenant` direct writes, `crate::execution` internals unless moved behind `nimbus-bridge`/service traits, `crate::service_registry` unless inverted. |
| Artifact contracts | `std::process::Command`, `ProcessArtifactVerifierCommandRunner`, Axum/router/server state, concrete storage providers, tenant authority relocation. |
| Provenance | `std::process::Command`, server adapter registries, `AppState`, router/listener lifecycle, process-backed verifier construction unless effect trait-owned. |
| Services | `crate::system_tenant` direct writes, `crate::local_enforcement` shim use, router/listener lifecycle, adapter modules, `AppState` in service core. |
| Operator | `AppState`, Axum middleware, routers, adapters, tenant workload execution, direct system-event persistence in pure value logic. |

The verifier skeleton now checks the baseline patterns that must already hold:
previous extracted crates stay server-free, no aggregate `nimbus-adapters`
crate exists, adapters do not import server-private `runtime_host`, and direct
adapter `_nimbus` upserts are absent.

## Initial Focused Test Set

The later phases may narrow these further, but the initial completion gate will
need focused coverage from these lanes:

- MongoDB: `cargo test -p nimbus-server mongodb -- --nocapture`
- Firebase/provider-family: `cargo test -p nimbus-server firebase -- --nocapture`
- Cloud Functions: `cargo test -p nimbus-server cloud_functions -- --nocapture`
- Convex: targeted Convex runtime, auth, subscriptions, deploy/scheduler, and
  host bridge test filters after the SSE1D sub-owner audit.
- Artifact effects: `cargo test -p nimbus-server artifact_verifier -- --nocapture`
- Provenance/runtime bundle admission: runtime bundle/provenance filters after
  SSE3 classification.
- Services: service manager, service registry, runtime service lookup, tenant
  isolation evidence, and HTTP service lifecycle filters after SSE4
  classification.
- Operator: local server security, local admin, local UI, deploy admin, and
  local audit filters after SSE5 classification.

## Verification Log

- `git status --short`: dirty worktree with active previous extraction files
  and unrelated work; no unrelated changes were reverted.
- `bash scripts/verify-server-system-bridge-adapters-extraction.sh`: passed
  12/12, including `cargo check --workspace`.
- `cargo tree -p nimbus-server --edges normal --depth 1`: passed and confirmed
  extracted crate dependencies listed in the import graph summary.
- `scripts/verify-server-seam-extraction-readiness.sh`: added as executable
  SSE verifier skeleton. It preserves the previous extraction gate and checks
  the SSE0 proof/control-plane baseline before SSE1A begins.
