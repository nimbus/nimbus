# SSE1C Cloud Functions Readiness Proof

Date: 2026-05-28
Status: completed

## Scope

Separate Cloud Functions protocol/runtime handling from server state where
practical, while recording the real runtime/provenance/service blockers that
still prevent a clean adapter extraction.

## Task Checklist

- [x] SSE1C.1 Audit Cloud Functions runtime/provenance effects.
- [x] SSE1C.2 Route runtime calls through `nimbus-bridge`.
- [x] SSE1C.3 Route app auth through `nimbus-auth`.
- [x] SSE1C.4 Preserve Cloud Functions behavior.
- [x] SSE1C.5 Record extraction decision for Cloud Functions.

## Cleanup Performed

- Replaced `crate::tenant` imports in Cloud Functions runtime execution with
  direct `nimbus_tenant` imports.
- Added `CloudFunctionsRuntimeContext` for HTTP runtime invocation so
  `execute_http_target` no longer accepts `Arc<AppState>`.
- Narrowed HTTP runtime execution to explicit capabilities:
  `Arc<nimbus_engine::Service>`, `Arc<dyn RuntimeServiceRegistry>`, and
  `TenantIsolationMode`.
- Updated callable and raw HTTP Cloud Functions paths to build this narrow
  runtime context at the transport boundary.
- Updated Cloud Functions Firebase Admin/trigger tests to consume the
  Firebase-owned Firestore model exports after SSE1B removed
  `provider_family`.

## Owner Classification

| Subtree | Owner classification | Notes |
| --- | --- | --- |
| `app_contract.rs`, manifests, target binding model | Cloud Functions protocol/model candidate | Pure-ish adapter contract code with manifest validation and authoring-surface rules. |
| `http/request.rs`, `http/response.rs` | Cloud Functions HTTP protocol candidate | Request/response shaping code independent of server state. |
| `http/invocation.rs` | Runtime execution candidate with server-owned capabilities | No longer accepts `AppState`; still depends on server runtime invocation plumbing and service-registry trait. |
| `http.rs`, `http/callable.rs`, `http/tenant.rs` | Server transport/auth composition | Axum `State<AppState>`, active deployment registry, callable auth verification, usage recording, and tenant binding resolution remain server-owned. |
| `execution.rs` trigger executor | Runtime execution candidate with service/provenance blockers | Already stores explicit service, registry, generation, service registry, and tenant mode; still calls server runtime invocation plumbing and provenance gate. |
| `host_bridge.rs` | Nimbus runtime bridge adapter | Uses `nimbus-bridge` `RuntimeHostContext`, `RuntimeHostScope`, `RuntimeHostInvocation`, and ABI helpers instead of server-private runtime host internals. |
| `runtime_api/firebase_admin/*` | Firebase Admin runtime extension candidate | Uses Firebase-owned Firestore model helpers and `nimbus-bridge` context; still belongs with Cloud Functions runtime bridge until runtime extension ownership is decided. |
| `registry.rs` | Runtime bundle/provenance composition | Owns Cloud Functions registry and runtime bundle provenance config, but still imports server execution provenance type. |

## Denied Dependency Audit

Command:

```bash
rg -n "crate::tenant|crate::provider_family|crate::runtime_host|AppState|DeploymentState|RouterBuildConfig|crate::application_auth|crate::system_tenant|std::process::Command" crates/nimbus-server/src/adapters/cloud_functions -g '*.rs'
```

Result summary:

- `http/invocation.rs` has no `AppState` or `crate::tenant` import after the
  cleanup.
- `execution.rs` has no `crate::tenant` import after the cleanup.
- No Cloud Functions file imports `crate::runtime_host`, `crate::provider_family`,
  or `crate::system_tenant`.
- Remaining `AppState`/`DeploymentState` hits are HTTP transport/auth
  composition in `http.rs`, `http/callable.rs`, and `http/tenant.rs`.
- Remaining `RouterBuildConfig` and `std::process::Command` hits are in tests
  that build server fixtures and generated Firebase/Framework bundle artifacts.
- Remaining server-private runtime/provenance hits are `crate::execution` and
  `RuntimeBundleProvenanceConfig`; these are blockers for extraction until
  SSE2/SSE3 classify artifact/provenance ownership.

## Authority And Boundary Notes

Tenant and runtime authority flow is explicit:

- HTTP runtime execution builds `TenantIsolationContext` through
  `nimbus-tenant`.
- Trigger runtime execution builds system tenant contexts through
  `nimbus-tenant`.
- Runtime host calls use `nimbus-bridge` context/scope/invocation APIs and ABI
  dispatch; there is no `crate::runtime_host` dependency.
- Application auth flows through `nimbus-auth` contracts and server-owned
  deployment resolver wrappers.

The remaining extraction blockers are real:

- `invoke_runtime_bundle_blocking_with_host`,
  `next_runtime_server_request_id`, and runtime error conversion still live in
  server `execution`.
- Runtime bundle provenance admission is still a server execution type and must
  be classified in SSE3 before Cloud Functions can move cleanly.
- `RuntimeServiceRegistry` is still server-owned and must be neutralized or
  moved in SSE4 before Cloud Functions runtime execution can be server-free.
- HTTP route entry, callable auth, deployment activation, and tenant binding
  resolution remain server composition.

## Decision

Cloud Functions is ready for partial extraction of protocol/model/manifest,
HTTP request/response shaping, and runtime extension model code. Full Cloud
Functions adapter extraction is blocked by runtime invocation/provenance and
service-registry seams that are intentionally scheduled for SSE2-SSE4.

Required next extraction shape:

- Extract protocol/model pieces only after runtime invocation and provenance
  ownership is decided.
- Keep Axum route handlers, active deployment lookup, callable auth resolution,
  generated artifact fixture process execution, and server runtime invocation
  plumbing in `nimbus-server`.
- Revisit Cloud Functions after SSE2/SSE3/SSE4 decide artifacts, provenance,
  and services.

## Verification Log

- `cargo test -p nimbus-server cloud_functions -- --nocapture`: passed.
  Unit-test target reported 39 passed, 0 failed, 727 filtered; `mongodb_spec`
  and `reactive_loop` integration targets had 0 matching filtered tests.
