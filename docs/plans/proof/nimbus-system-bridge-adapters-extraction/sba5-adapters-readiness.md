# SBA5 Adapters Readiness Proof

Date: 2026-05-28
Status: completed

## Scope

Prepare external compatibility adapters for extraction without turning
`nimbus-adapters` into a second server crate.

## Candidate Modules

- `crates/nimbus-server/src/adapters/convex`
- `crates/nimbus-server/src/adapters/firebase`
- `crates/nimbus-server/src/adapters/cloud_functions`
- `crates/nimbus-server/src/adapters/mongodb`
- Provider-family helpers that exist only to serve adapter protocols

## Readiness Questions

- Which adapter imports are true composition needs versus accidental server
  reach-through?
- Which server-owned wrappers must remain in `nimbus-server` until route
  registration and listener lifecycle are inverted?
- Should extraction use one aggregate `nimbus-adapters` crate first, or
  separate per-adapter crates?
- Which tests move with adapter protocol code, and which remain server
  integration tests?

## Forbidden Shape

SBA5 is not complete if the planned adapter extraction would require
`nimbus-adapters` to own:

- `AppState`
- router/listener lifecycle
- local admin/operator authority
- `_nimbus` persistence implementation
- server deployment activation
- runtime bridge internals instead of `nimbus-bridge`
- application-auth internals instead of `nimbus-auth` or server-supplied
  transport/deployment wrappers

## Task Checklist

- [x] SBA5.1 Audit adapter server-private imports.
- [x] SBA5.2 Identify required composition traits; do not introduce no-consumer
      traits after the extraction decision rejected this phase.
- [x] SBA5.3 Decide aggregate versus per-adapter crates.
- [x] SBA5.4 Keep listener/state ownership in server.
- [x] SBA5.5 Preserve adapter test ownership.

## Verification Log

- `rg -o "crate::(state|router|local_server|system_tenant|application_auth|runtime_host|service_registry|execution|tenant|provider_family|sandbox|license|machine|http|error_envelope|protocol|ws)" crates/nimbus-server/src/adapters -g '*.rs' | sed 's/^.*crate::/crate::/' | sort | uniq -c`
  reported:
  - 31 `crate::tenant`
  - 27 `crate::state`
  - 25 `crate::local_server`
  - 18 `crate::system_tenant`
  - 17 `crate::execution`
  - 10 `crate::service_registry`
  - 10 `crate::router`
  - 8 `crate::application_auth`
  - 7 `crate::ws`
  - 5 `crate::provider_family`
  - 2 `crate::protocol`
- Direct `crate::runtime_host` imports are gone after SBA4.
- Shared auth contract imports now come from `nimbus_auth`; remaining
  `crate::application_auth` imports are server transport/deployment wrappers.

## Required Composition Interfaces

The audit shows the following interfaces would be required before adapter code
can move without importing `nimbus-server`:

| Interface | Current server-private dependency | Extraction requirement |
| --- | --- | --- |
| Adapter state snapshot | `AppState`, `DeploymentState` | Explicit per-request context with service, tenant isolation mode, registries, and deployment generation, not global server state. |
| Local route authorization and audit | `local_server` | Server-owned route gate that passes a narrow admitted route/audit context into adapter handlers. |
| System evidence | `system_tenant` shim | Direct `nimbus-system` writer interfaces supplied by server composition; no adapter direct `_nimbus` persistence. |
| Runtime invocation orchestration | `execution::invocations`, `execution::host_calls`, `execution::subscriptions` | Separate provenance/runtime-invocation interfaces; adapters use `nimbus-bridge` for host capabilities and a server-supplied invocation runner for bundle execution. |
| Runtime service lookup | `service_registry` | Narrow service-registry trait owned outside server or supplied by server composition. |
| Tenant admission | `tenant` shim | Direct `nimbus-tenant` imports or admitted decisions supplied by server, depending on adapter surface. |
| WebSocket protocol | `ws` | Server-owned WebSocket handshake/upgrade wrapper with adapter-owned frame handling behind an explicit socket session interface. |
| Transport auth | `application_auth` wrappers | Keep header/metadata/deployment extraction in server; pass `nimbus-auth` results into adapter code. |
| Provider-family translation | `provider_family::firestore` | Move with adapters only after confirming the helpers are adapter/protocol translation, not core data model. |

Introducing these traits without moving consumers would add architecture-shaped
bloat. The correct readiness output is therefore the interface inventory and a
reject decision for aggregate extraction in SBA6.

## Aggregate Versus Per-Adapter Decision

Reject aggregate `nimbus-adapters` extraction for this wave.

Rationale:

- Convex combines HTTP, WebSocket, runtime invocation, subscriptions, system
  evidence, scheduler state, local audit, and deployment auth.
- Firebase combines REST, gRPC, WebSocket listen, Firestore-family translation,
  application auth, and tenant admission.
- Cloud Functions combines HTTP/callable surfaces, generated artifact loading,
  trigger execution, runtime invocation, and Firebase Admin runtime APIs.
- MongoDB is comparatively self-contained, but still imports tenant admission.

An aggregate crate today would either depend on `nimbus-server` or recreate
server composition behind a different name. That is a trust regression.

Follow-on extraction should be per-adapter or staged by dependency shape:

1. MongoDB adapter readiness first, because it has the smallest server surface.
2. Firebase provider-family/helper extraction next, after transport auth and
   tenant admission inputs are narrowed.
3. Cloud Functions extraction after runtime invocation/provenance interfaces
   are separated.
4. Convex extraction last, after WebSocket/session/system-evidence interfaces
   are inverted.

## Test Ownership Decision

- Adapter protocol/unit tests stay with the adapter candidate in a future
  per-adapter extraction.
- Server integration tests that build `RouterBuildConfig`, exercise
  `AppState`, local-server route gates, deployment activation, WebSocket
  upgrades, or `_nimbus` evidence stay in `nimbus-server`.
- Existing focused adapter tests remain the verification source for this
  readiness decision; no code move occurred in SBA5.

## Readiness Decision

Do not extract `nimbus-adapters` in this wave. Proceed to SBA6 with a recorded
keep/reject decision instead of a decorative aggregate crate.
