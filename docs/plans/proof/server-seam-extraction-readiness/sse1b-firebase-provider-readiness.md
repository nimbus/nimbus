# SSE1B Firebase And Provider-Family Readiness Proof

Date: 2026-05-28
Status: completed

## Scope

Clean the Firebase/Firestore adapter seam enough that reviewers can separate
protocol/model/operation code from server transport, auth resolution, and route
composition.

## Task Checklist

- [x] SSE1B.1 Audit provider-family helpers.
- [x] SSE1B.2 Isolate Firebase server state and auth where practical.
- [x] SSE1B.3 Preserve Firebase REST/gRPC/listen behavior.
- [x] SSE1B.4 Record extraction decision for Firebase/provider-family.

## Cleanup Performed

- Moved Firestore model helpers from the root `provider_family` module into
  `crates/nimbus-server/src/adapters/firebase/firestore_model.rs`.
- Removed `mod provider_family` from `crates/nimbus-server/src/lib.rs`.
- Updated Cloud Functions Firebase Admin and execution code to consume
  Firebase-owned Firestore model exports instead of `crate::provider_family`.
- Changed `adapters/firebase/operations.rs` to accept `&Arc<nimbus_engine::Service>`
  instead of `&Arc<AppState>`.
- Changed Firestore operation authority to import
  `nimbus_tenant::TenantIsolationContext` directly.
- Changed Firebase gRPC write/listen stream session state to hold
  `Arc<nimbus_engine::Service>` instead of the whole `AppState`; auth and
  transport entrypoints still use `AppState` for deployment/auth composition.

## Owner Classification

| Subtree | Owner classification | Notes |
| --- | --- | --- |
| `adapters/firebase/firestore_model.rs` | Firebase adapter model candidate | Firestore path parsing, default database validation, and storage table mapping. Shared with Cloud Functions through Firebase exports. |
| `adapters/firebase/*_request.rs`, `resource_names.rs`, `serializer.rs`, `response.rs`, `errors.rs` | Firebase protocol/model candidate | Request lowering, response shaping, resource names, and error/status mapping. No server state needed. |
| `adapters/firebase/operations.rs` | Firebase operation candidate with engine capability | Uses `nimbus-core`, `nimbus-tenant`, and explicit `Arc<nimbus_engine::Service>`. No `AppState`. |
| `adapters/firebase/grpc/write_stream.rs` stream core | Firebase streaming operation candidate | Active stream state now holds engine service plus tenant context, not full server state. |
| `adapters/firebase/grpc/listen_stream.rs` stream core | Firebase streaming operation candidate | Active stream state now holds engine service plus retained-listen registry, not full server state. |
| `adapters/firebase/mod.rs` REST handlers | Server transport/composition | Axum route handlers still accept `State<Arc<AppState>>`, resolve deployment auth, gate adapter enablement, and record usage. |
| `adapters/firebase/grpc/mod.rs`, `grpc/unary.rs`, `listen_websocket.rs` entrypoints | Server transport/composition | Tonic service construction, auth extraction, route/websocket transport, and deployment state stay server-owned for now. |

## Denied Dependency Audit

Commands:

```bash
rg -n "crate::application_auth|AppState|DeploymentState|RouterBuildConfig|crate::system_tenant|crate::local_server|crate::runtime_host|crate::provider_family|crate::tenant|std::process::Command" crates/nimbus-server/src/adapters/firebase -g '*.rs'
rg -n "provider_family|mod provider_family" crates/nimbus-server/src -g '*.rs'
```

Result summary:

- `operations.rs` has no `AppState`, `crate::tenant`, `crate::provider_family`,
  `crate::application_auth`, `crate::system_tenant`, `crate::local_server`,
  `crate::runtime_host`, router, or process execution imports.
- The root `provider_family` module is no longer imported or declared.
- Remaining `AppState` hits are transport/composition owned: REST Axum
  handlers, Firestore gRPC service construction, unary auth resolution,
  websocket auth resolution, and adapter enablement checks.
- Remaining `crate::application_auth` hits are server-owned auth-resolution
  wrappers because they need the active deployment verifier and Firebase
  emulator mock-token policy.
- No Firebase adapter file imports `crate::system_tenant`, `crate::local_server`,
  `crate::runtime_host`, `crate::provider_family`, `crate::tenant`,
  `RouterBuildConfig`, or `std::process::Command`.

## Authority And Boundary Notes

Tenant separation remains enforced before operation access:

- `tenant_context_for_database` constructs a `TenantIsolationContext` from the
  Firestore database project id and authenticated principal.
- `tenant_id_for_context_database` rejects mismatched database/tenant context
  before reads, writes, queries, transactions, collection-id listing, and
  aggregations.
- Focused tests cover rejected cross-tenant bearer principals for REST, unary
  gRPC, write stream, and websocket listen surfaces.

The remaining blocker for full Firebase crate extraction is transport/auth
composition, not Firestore operation ownership:

- REST and gRPC entrypoints still need `AppState` to resolve the active
  deployment auth verifier, Firebase emulator mock-user-token policy, adapter
  enablement, and authenticated usage recording.
- A future extraction should introduce a `FirebaseAuthResolver`/usage-recorder
  capability and a transport-owned adapter enablement gate before moving REST
  and gRPC entrypoints.

## Decision

Firebase/provider-family is ready for partial extraction of model, request,
response, serializer, error, and operation code behind explicit engine and auth
capabilities. Full Firebase adapter extraction is blocked until the server
transport/auth composition is inverted.

Required next extraction shape:

- Extract Firestore model/protocol/operation code without `nimbus-server`.
- Keep Axum route mounting, Tonic service state construction, websocket
  transport, deployment auth resolution, adapter enablement, and usage
  recording in `nimbus-server` until those are exposed as narrow traits.
- Do not create aggregate `nimbus-adapters`; Firebase and MongoDB have
  different remaining blockers and should remain per-adapter decisions.

## Verification Log

- `cargo test -p nimbus-server firebase -- --nocapture`: passed. Unit-test
  target reported 142 passed, 0 failed, 624 filtered; `mongodb_spec` and
  `reactive_loop` integration targets had 0 matching filtered tests.
