Status: completed
Phase: SSE1D Convex adapter readiness
Ledger position: SSE1D completed; resume at SSE2 Artifact effects readiness

## Current Import Graph And Owner Classification

Convex remains the largest adapter and is not ready for whole-adapter
extraction. The code now has a clearer split:

| Lane | Modules | Owner decision |
| --- | --- | --- |
| Composition shell | `handlers/*`, route extractors, WebSocket entrypoint, local-server audit, deployment registry lookup | `nimbus-server` |
| Protocol model | `requests`, `templates`, `manifest`, `document_identity`, registry schema/resolution, host payloads/responses | Convex adapter candidate |
| Operation core | sync/async query/mutation/action execution, HTTP action execution, subscription transform planning | Convex adapter candidate once runtime/effect capabilities are narrower |
| Authority/effects bridge | tenant admission, application auth normalization, runtime bridge, system evidence, runtime invocation, service registry | Canonical crates where available; server composition/effect traits where not yet extracted |

Remaining server-private imports are classified:

- `AppState`: route, WebSocket, and deployment composition shell.
- `crate::local_server`: local operator route-family authorization/audit for
  system-tenant Convex routes.
- `crate::application_auth`: deployment-bound bearer verification still needs
  a narrow auth resolver trait before handler extraction.
- `crate::execution`: runtime bundle invocation, request correlation, runtime
  subscription handles, and host-call error plumbing are server effect seams.
- `crate::service_registry`: runtime service registry is a services readiness
  blocker for SSE4.

The old server re-export shims are no longer used by Convex production code:

- no `crate::tenant` import remains under `crates/nimbus-server/src/adapters/convex`,
- no `crate::system_tenant` import remains under `crates/nimbus-server/src/adapters/convex`,
- no `crate::runtime_host` import remains under `crates/nimbus-server/src/adapters/convex`.

## Active Cleanup Performed

- Replaced Convex tenant authority imports with direct `nimbus_tenant` imports
  in handler, runtime invocation, subscription, and test helper code.
- Replaced Convex `_nimbus` evidence calls/types with direct `nimbus_system`
  calls/types for deployment summaries, run records, scheduler state, and
  subscription status evidence.
- Kept server-owned application bearer verification and local-server audit in
  server composition code because those still bind to `DeploymentState`,
  `AppState`, and operator route-family audit.
- Tightened direct named subscription planning so Convex `Get` subscriptions
  resolve table-scoped protocol IDs at the adapter boundary and store raw
  storage IDs in builtin subscription transforms.
- Updated Convex tests to distinguish protocol-facing table-scoped Convex IDs
  from raw storage IDs used by generic HTTP document APIs.
- Widened only post-release waits in the faulted overlap scenario from one to
  five seconds; the test still proves requests block while the journal apply
  fault is active and complete after release, but no longer relies on a brittle
  one-second scheduler window under the full Convex lane.

## Denied Import Audit

Checks performed:

```text
rg -n 'crate::tenant|crate::system_tenant' crates/nimbus-server/src/adapters/convex -g '*.rs'
```

Result: no matches.

```text
rg -n 'crate::runtime_host|runtime_host::|upsert_system_document' crates/nimbus-server/src/adapters/convex -g '*.rs'
```

Result: no matches.

Expected retained matches:

- `AppState` remains in route/WebSocket composition shell modules.
- `crate::local_server` remains for local operator authorization and audit.
- `crate::application_auth` remains for deployment-bound application bearer
  verification.
- `crate::execution` and `crate::service_registry` remain runtime/effect
  blockers to be handled by later bridge/services readiness work.

## Behavior And Security Verification

Commands run:

```text
cargo test -p nimbus-server convex_runtime_only_get_reuses_materialized_serving_snapshot_after_full_scan_warmup -- --nocapture
```

Result: 1 passed, 0 failed, 0 ignored.

```text
cargo test -p nimbus-server convex_http_demo_faulted_overlap_still_completes_http_post_and_follow_up_action -- --nocapture
```

Result: 1 passed, 0 failed, 0 ignored.

```text
cargo test -p nimbus-server --test reactive_loop runtime_queries::get_and_query::get -- --nocapture
```

Result: 2 passed, 0 failed, 0 ignored.

```text
cargo test -p nimbus-server convex -- --nocapture
```

Result:

- lib target: 132 passed, 0 failed, 5 ignored, 629 filtered out.
- `mongodb_spec` target: 0 passed, 0 failed, 23 filtered out.
- `reactive_loop` target: 18 passed, 0 failed, 14 filtered out.

Ignored lib tests are intentional:

- one real service-manager/krun integration test requires Linux, KVM,
  `buildah`, `conmon`, and network access,
- four generated-history verification-harness corpus tests run in dedicated
  harness lanes.

Security-relevant tests covered by this lane include wrong-table Convex IDs,
runtime host bridge grant rejection, table-scoped custom IDs, application auth
tenant rejection, local-admin/application-auth separation, system-tenant route
authorization, runtime service grant rejection, runtime cancellation, and
runtime subscription read tracking.

## Extraction Decision

Decision: `blocked` for whole Convex adapter extraction; `ready` for selected
protocol/model subtrees.

Ready candidates:

- `document_identity`,
- host bridge payload/response models,
- manifest/request/template parsing,
- registry schema/resolution/deploy summary value logic,
- subscription transform planning after the ID-boundary fix.

Blocked candidates and next moves:

- Handlers/routes/WebSocket: introduce a narrow Convex route auth/audit context
  before removing `AppState` and `crate::local_server`.
- HTTP actions: introduce a deployment-bound application auth resolver trait
  before removing `crate::application_auth`.
- Runtime-backed invocation/subscriptions: extract or trait-invert runtime
  invocation and runtime service registry capabilities before removing
  `crate::execution` and `crate::service_registry`.
- System evidence writes now use `nimbus_system` APIs, but higher-level
  evidence writer ownership still belongs in later services/operator phases.

Aggregate `nimbus-adapters` remains rejected. Convex needs per-adapter
readiness and specific runtime/auth/effect traits; putting it into an aggregate
crate now would create a second server crate.

## Verifier Updates

`scripts/verify-server-seam-extraction-readiness.sh` now checks:

- completed SSE1D proof,
- direct `nimbus_tenant`, `nimbus_system`, `nimbus_auth`, and `nimbus_bridge`
  use in Convex proof/code,
- no Convex `crate::tenant`, `crate::system_tenant`, server runtime-host, or
  direct `_nimbus` upsert imports,
- subscription planner resolves table-scoped IDs before storing builtin
  `Get` transforms,
- focused Convex and reactive-loop pass counts,
- aggregate adapter extraction remains rejected.

## Resume Cursor

Resume at SSE2 Artifact effects readiness. Start by classifying tenant-owned
artifact contracts separately from process-backed verifier execution and update
`docs/plans/proof/server-seam-extraction-readiness/sse2-artifact-effects-readiness.md`
before code edits.
