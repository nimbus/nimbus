# Plan: Tenant Isolation Control Plane

Active plan for making tenant isolation an explicit control-plane contract
across in-process runtime compute, microVM service compute, networking,
storage, runtime host access, and sandbox artifacts.

This plan exists because hardware microVM isolation is necessary but not
sufficient. OCI gives Nimbus an execution envelope; Nimbus must still prove
that tenant identity, admission policy, host capabilities, runtime grants,
network exposure, storage roots, and cleanup all stay tenant-scoped.

---

## Status

- **Status:** `active`
- **Activated:** 2026-05-22
- **Primary owner:** this plan
- **Parent references:**
  - `docs/plans/security/sandbox-isolation-audit.md`
  - `docs/architecture/sandbox/microvm-service-baseline.md`
  - `docs/architecture/runtime/permission-model.md`
  - `docs/architecture/storage/provider-topologies.md`
  - `docs/architecture/server/auth-runtime-trust.md`

## Isolation Contract

Tenant isolation is not satisfied by "one microVM per service" alone.

For production multi-tenant claims, every tenant workload must have a
server-owned `TenantIsolationContext` or equivalent carrying:

- `tenant_id`
- deployment or bundle identity
- sandbox/service identity when a service is involved
- authenticated principal or operator authority
- execution tier, runtime backend, compatibility target, and runtime policy
- admitted runtime grants and service grants
- admitted network exposure policy
- admitted storage, volume, secret, image, and resource policy
- admitted compute, concurrency, memory, disk, log, and port budgets

No tenant-controlled input may lower directly to host paths, OCI mounts,
devices, capabilities, network listeners, service bindings, secrets, or global
runtime grants. Admission must either reject the input or rewrite it into a
Nimbus-owned tenant-scoped resource before any OCI bundle, runtime invocation,
storage operation, or service endpoint is materialized.

## Enforcement Seam Map

Tenant isolation must be enforced at the interfaces where tenant-controlled
intent becomes host authority. The current codebase has the right rough seams,
but the plan must close the missing gates at those seams rather than pushing
checks into leaf operations.

| Seam | Code owner today | Tenant-isolation rule |
| --- | --- | --- |
| Transport tenant admission | `crates/nimbus-server/src/router.rs`, `crates/nimbus-server/src/http/*`, adapter handlers, WebSocket tenant extraction | Before a handler calls `nimbus_engine::Service` or a runtime registry with a path/header tenant, it must hold an authorized tenant access context for that tenant. |
| Runtime invocation admission | `RuntimeInvocationContext`, `RuntimeHostScope`, `RuntimePolicy`, `RuntimeGrants` | In-process V8/Deno, future Bun/JSC, and future WASM invocations consume the same tenant isolation context. Backend/compatibility selection never widens grants or tenant reach. |
| Runtime host-call ABI | `RuntimeCapabilityHost`, Convex HostBridge, shared runtime host-call ops | Host calls use the server-owned invocation tenant and exact grants. JavaScript payloads cannot supply or override `tenant_id`. |
| Runtime OS capability boundary | `crates/nimbus-runtime/src/runtime_capabilities.rs`, Node/Deno permissions, worker/run/ffi/sys ops | Filesystem, env, network, subprocess, workers, FFI, and sys grants are enforced before the engine touches host resources. Generic loopback network grants are a cross-service authority and must be constrained for production. |
| Service binding and microVM lifecycle | `SandboxServiceManager`, `RuntimeServiceRegistry`, `SandboxBackend`, `SandboxSpec` | Service lookup and launch are keyed by tenant plus service; an admitted sandbox spec must match the requested tenant/service/backend before launch. |
| OCI and sandbox artifact materialization | `nimbus-sandbox` krun/container backends, OCI materializer/builders | Tenant-controlled service intent cannot become OCI mounts, devices, ports, rootfs paths, logs, or state until admission rewrites it into tenant-owned resources. |
| Storage namespace selection | `PersistenceProvider`, `TenantPersistence`, provider-specific opened tenants | Physical namespace isolation remains provider-owned: embedded tenant files, external schemas/databases, and provider metadata. This seam does not replace the transport tenant authorization gate. |
| System/control data | `system_tenant`, local-server access middleware, metadata routes | `_nimbus` is operator/control-plane data. Tenant API/runtime surfaces must not treat it as a user tenant. |

## Current Audit

Reviewed on 2026-05-22.

| Layer | Current evidence | Gap before production tenant isolation |
| --- | --- | --- |
| Tenant identity | `TenantIsolationContext` now carries tenant, authority, surface, runtime bundle tenant labels, deployment generation, and service launch validation across native HTTP, native WebSocket, Convex, Firebase/Firestore, Cloud Functions, MongoDB compatibility commands, runtime HostBridge, and sandbox service launch. `SandboxServiceManager` keys active handles by `(tenant_id, service_name)` and rejects launch specs whose tenant/service/backend differ from the admitted request. | Later phases must extend the context/admission artifact with admitted network, storage, volume, image, secret, runtime grant, and quota policy instead of passing those as independent ad hoc arguments. |
| Storage database | Embedded providers use per-tenant storage files; Postgres uses one tenant schema per tenant behind provider metadata; MySQL uses one tenant database per tenant; external provider docs require per-tenant namespaces; `_nimbus` is a reserved system tenant. Application principals carrying tenant claims are now checked by `TenantIsolationContext` before Convex, Convex HTTP actions, Cloud Functions HTTP, and Firebase/Firestore database contexts reach storage/runtime. Convex HTTP query, Firebase REST `batchGet`, and Cloud Functions callable transport tests now prove same-tenant access succeeds while a `tenant_id=tenant-b` bearer targeting tenant `tenant-a` returns `403`. | Physical namespace selection is mostly at the right provider seam, but native HTTP/API tenant authorization is not yet a full tenant-membership/session gate for every non-operator surface. Anonymous/public app requests remain allowed when the app exposes them, and principal-less native HTTP remains local-operator scoped. |
| Sandbox state | krun/container bundle, manifest, conmon state, logs, persist/exit files, materialized rootfs roots, and container network namespace/status/IPAM state now lower under tenant-owned roots. State views and port scans enumerate tenant roots, and tenant teardown removes only the target tenant's sandbox artifact roots while leaving shared content-addressed image cache and other tenants intact. | Named volume admission/paths are still not lowered into service bundles and remain TIC6. |
| Networking | `SandboxPortBinding` defaults to loopback; krun bundles emit address-bearing `krun.port_map`; patched `nimbus-crun`/`nimbus-libkrun` proved localhost-only TSI binding on the rootful Linux service path after the quota changes. Runtime service lookup is tenant-scoped. Compose service lowering now rejects non-loopback host addresses unless a future operator network exposure policy is added. System port records now carry explicit tenant, service, and endpoint ownership fields. krun/container port allocation now enforces a default per-tenant published-port quota while keeping host-port reservation global. Container network/IPAM mutable state is tenant-rooted. `TenantIsolationMode::Production` now rejects generic loopback/wildcard in-process runtime network grants before Convex or Cloud Functions runtime invocation. | Future admitted tenant-owned endpoint grants need an explicit policy object instead of generic localhost authority. |
| In-process runtime compute | `RuntimeLimits` already has per-tenant active/in-flight/queued top-level invocation caps; `RuntimePolicy` owns runtime instance concurrency; runtime permissions are grant-derived; `RuntimeInvocationContext` and HostBridge carry the invocation tenant. `TenantIsolationContext::ensure_runtime_policy_admitted(...)` now gates production `in_process_untrusted` runtime policies for Convex and Cloud Functions, rejecting generic localhost/wildcard networking, listen grants, run, FFI, env write, identity, tool, worker, inspector, privileged mode, non-application presets, and broad filesystem/package-loading roots before JavaScript runs. `TenantIsolationMode::default()` is production, so public `serve*`, router builders, and CLI `nimbus start` enter production tenant isolation unless they explicitly opt out; `nimbus dev` opts into local-development mode. Active/in-flight/queued per-tenant runtime budgets are first-class start flags. Production rejections now name the canonical fallback tier: `in_process_trusted_only`, `microvm_service`, or future `wasm_capability_sandbox`. | TIC4 still needs actual lowering/routing for rejected workloads, native-addon/package-manager proof beyond grant shape, and tenant-accounted CPU/memory/time/worker budgets beyond top-level invocation accounting. |
| MicroVM service compute | krun launches one service sandbox as one microVM; service manager does not intentionally share a VM across tenants. Runtime permission model separates `in_process_untrusted` from `microvm_service`. | Need hard admission tests that prevent multiple tenants sharing a guest, enforce per-tenant/per-sandbox quotas, and prove workloads with broad OS needs move to `microvm_service` or a trusted tier. |
| Volumes/files | Compose volumes are parsed/rendered but not admitted into the krun bundle; current bundle only adds Nimbus-owned read-only helper mounts. | Bind mounts must be rejected by default. Named volumes need Nimbus-owned paths under the tenant root, explicit read/write policy, quota, cleanup, and no cross-tenant reuse unless a future shared-read-only artifact policy is added. |
| Images/builds | OCI materializer verifies pulled blob digests and sanitizes archive paths before extraction. | Production image admission still needs digest-pinned references and provenance/signature policy before tenant-controlled images are mounted or extracted. |
| System/control data | `_nimbus` records services, ports, jobs, machines, and events with tenant IDs for operator visibility. | Tenant runtime/API surfaces must not expose `_nimbus` as a tenant. Operator-only control-plane reads need explicit auth and audit, not tenant path access. |

## Phase Ledger

| Phase | Status | Goal | Verification |
| --- | --- | --- | --- |
| TIC0 | `done` | Audit current tenant-isolation shape and define production gates. | This plan records code/doc evidence and gaps. |
| TIC1 | `done` | Add an explicit tenant isolation context/admission artifact. | Unit tests prove mismatched tenant/deployment/service/runtime identities are rejected before runtime or sandbox launch. |
| TIC2 | `done` | Tenant-scope existing sandbox filesystem state. | krun/container bundle/state/rootfs/log paths include tenant-owned roots; tenant deletion stops services and removes tenant sandbox artifacts without touching other tenants. |
| TIC3 | `done` | Fail-closed network admission and port ownership. | Non-loopback service exposure is rejected unless operator policy allows it; port leases carry tenant/service identity, quotas, and cleanup; localhost-only proof remains green on the rootful Linux service path. |
| TIC4 | `in_progress` | Runtime compute admission and host capability isolation. | CLI start enters production isolation by default; production runtime policies reject unsafe tier/backend/grant combinations; Node loopback grants cannot bypass service grants; runtime CPU/memory/time/worker/nested-call budgets are tenant-accounted. |
| TIC5 | `in_progress` | Tenant-scoped storage/API authorization. | Native HTTP, adapter, runtime, scheduler, and system-control paths prove a principal/session cannot address another tenant by swapping the path tenant ID. |
| TIC6 | `pending` | Tenant-scoped volumes, images, secrets, and mounts. | Bind mounts are denied by default; named volumes lower only to Nimbus-owned tenant paths; production images require digest/provenance policy; secrets are handles, not ambient env. |
| TIC7 | `pending` | Per-tenant microVM/resource quotas. | Tests cover per-tenant active microVM count, sandbox CPU/memory/disk/log/port quotas, scheduler fairness, and runtime in-flight limits. |
| TIC8 | `pending` | End-to-end multi-tenant proof harness. | A two-tenant harness proves runtime compute, microVM compute, network, storage, HostBridge, service lookup, volumes, logs, cleanup, and system metadata isolation. |

## Required Gates

### Gate A: Admission Before OCI

Admission must run before `config.json` or a materialized rootfs exists.

Acceptance criteria:

- compose service lowering rejects unsupported privileged controls instead of
  merely warning when running in production mode
- host bind mounts are rejected unless an explicit operator policy admits a
  specific read-only host path
- non-loopback port exposure requires an explicit policy decision that records
  tenant, service, host address, host port, guest port, and reason
- generated `SandboxSpec` cannot contain a tenant ID different from the
  tenant request being served

### Gate B: Tenant-Owned Sandbox Roots

Production sandbox artifacts must be rooted by tenant before sandbox:

```text
<runtime-root>/tenants/<tenant_id>/sandboxes/<sandbox_id>/bundle/config.json
<runtime-root>/tenants/<tenant_id>/sandboxes/<sandbox_id>/rootfs
<runtime-root>/tenants/<tenant_id>/sandboxes/<sandbox_id>/state
<runtime-root>/tenants/<tenant_id>/volumes/<volume_id>
```

Shared immutable caches are allowed only for content-addressed blobs that are
opened read-only after digest verification. Mutable rootfs, logs, volumes,
runtime state, and manifests must never be shared writable state.

Acceptance criteria:

- two tenants declaring the same service name get distinct bundle, state,
  rootfs, log, and volume paths
- tenant deletion removes only that tenant's sandbox artifacts and ports
- state inspection by sandbox ID also verifies tenant ownership on any
  tenant-facing path

### Gate C: Network Isolation

Default service exposure is loopback-only and mediated by Nimbus.

Acceptance criteria:

- default Compose `HOST:CONTAINER` lowers to `127.0.0.1:HOST:CONTAINER`
- `0.0.0.0`, LAN IPs, and non-loopback IPv6 addresses fail closed unless
  operator policy admits them
- `ctx.services` and `ctx.services.get()` remain tenant-scoped
- in-process runtimes without explicit network grants cannot connect to
  localhost service ports directly
- granting generic localhost network access is treated as a cross-service
  capability and must be incompatible with production tenant isolation unless
  the grant is constrained to admitted tenant-owned endpoints

### Gate D: Runtime Compute And HostBridge Isolation

In-process runtime execution is a compute boundary and must be admitted
independently from microVM service placement.

Acceptance criteria:

- runtime invocation construction consumes the tenant isolation context rather
  than a free `TenantId` plus independent policy pieces
- `RuntimePolicy::normalized()` or the server-side policy construction path
  rejects production `in_process_untrusted` policies with `run`, `ffi`,
  native-addon, unconstrained package-loading, broad filesystem, or generic
  localhost network authority
- Node-compatible defaults do not automatically make every loopback service
  reachable; service grants and network grants compose through admitted
  tenant-owned endpoints
- runtime instance, active invocation, in-flight invocation, queued invocation,
  nested call, worker-thread, heap, and execution-time budgets are accounted
  per tenant
- HostBridge operations never accept a tenant ID from JavaScript payloads; they
  use the server-owned invocation tenant only

### Gate E: Storage And API Isolation

Storage isolation must be enforced at both the storage locator and the access
boundary.

Acceptance criteria:

- embedded storage remains one tenant file per tenant; external storage remains
  one tenant schema/database/namespace per tenant
- all native HTTP, adapter, scheduler, and runtime host operations bind the
  request principal/session to the tenant before calling the engine
- tenant code cannot read or mutate `_nimbus`
- tenant creation, deletion, schema, journal, document, query, scheduling,
  service lifecycle, adapter, WebSocket, and metadata routes all pass through
  the same tenant access decision instead of open-coding path tenant parsing

### Gate F: MicroVM Compute Quotas And Placement

One tenant's compute must not exhaust another tenant's ability to run.

Acceptance criteria:

- one microVM/service sandbox belongs to exactly one tenant
- no production mode places multiple tenants in one guest
- per-tenant active sandbox, CPU, memory, disk, log, port, scheduler, and
  runtime in-flight limits are enforceable and tested
- untrusted workloads needing subprocesses, native addons, broad networking,
  or broad filesystem access are rejected from `in_process_untrusted` and
  moved to `microvm_service` or `in_process_trusted_only`

## Verification Harness

TIC8 should add a focused two-tenant harness that creates tenants `tenant-a`
and `tenant-b`, starts identically named services for both, and asserts:

- service IDs, sandbox IDs, bundle paths, rootfs paths, state dirs, logs, and
  named volumes are distinct
- each tenant sees only its own `ctx.services` binding
- direct runtime calls cannot request the other tenant's service binding
- default service ports are loopback-only
- a runtime without network grant cannot connect to either service port
- a runtime with a service grant but no admitted network endpoint receives only
  the scoped binding exposed by `ctx.services`, not generic localhost reach
- storage writes and queries cannot cross tenants by swapping IDs
- native HTTP and adapter routes reject a verified principal/session attempting
  to address another tenant by swapping the path tenant
- deleting `tenant-a` stops and removes only `tenant-a` services/artifacts
  while `tenant-b` remains reachable
- `_nimbus` records are visible only through operator-authenticated surfaces

## Progress Checkpoints

### 2026-05-22 TIC1 Context Baseline

Implemented the first server-owned tenant isolation artifact and wired it
through the highest-risk existing execution seams.

Completed in this checkpoint:

- Added `TenantIsolationContext` plus service-scoped validation for tenant,
  service name, and sandbox backend before a service launch reaches a sandbox
  backend.
- Routed native HTTP tenant parsing through a single helper that produces an
  operator `TenantIsolationContext` before document, query, schema, scheduler,
  service lifecycle, tenant, and metadata handlers call the service layer.
- Routed native WebSocket tenant admission through an operator
  `TenantIsolationContext` before tenant existence checks and socket handling.
- Routed Convex query, mutation, action, scheduling, WebSocket, runtime
  invocation, HostBridge, runtime-backed subscription setup, and HTTP-action
  dispatch through an application/operator `TenantIsolationContext`.
- Routed Cloud Functions HTTP and trigger execution through
  `TenantIsolationContext` before `RuntimeHostScope` construction.
- Updated `RuntimeHostScope`, `ConvexHostBridgeScope`, and
  `RuntimeInvocationContext` so runtime host operations and service snapshots
  use the server-owned invocation tenant.

Verification evidence:

- `cargo check -p nimbus-server`
  - result: pass; `Finished dev profile` in 5.31s
- `cargo test -p nimbus-server tenant_isolation -- --nocapture`
  - result: pass; 3 passed, 0 failed, 700 filtered out
- `cargo test -p nimbus-server service_manager -- --nocapture`
  - result: pass; 7 passed, 0 failed, 696 filtered out
- `cargo test -p nimbus-server convex -- --nocapture`
  - result: pass; 116 passed, 0 failed, 5 ignored, 582 filtered out
  - `tests/reactive_loop.rs`: 18 passed, 0 failed, 14 filtered out

Remaining before TIC1 is done:

- Firebase/Firestore REST, unary gRPC, listen stream, and write stream still
  derive tenant identity directly from Firestore database names. They need the
  same context artifact and adapter authority binding before provider calls.
- Native and adapter request auth now flows through the context, but tenant
  membership/role policy is still not a global access decision. TIC5 must
  close that as an authorization rule, not only an identity-carrier shape.
- Runtime grant and execution-tier admission is not complete. TIC4 still needs
  production-mode rejection for unsafe in-process grants and per-tenant budget
  enforcement beyond the existing invocation counters.
- The context currently carries tenant, authority, and surface. Later TIC
  phases should extend or wrap it with admitted deployment, runtime, network,
  storage, volume, image, secret, and quota policy instead of adding parallel
  ad hoc arguments.

### 2026-05-22 TIC1 Firebase/Firestore Context Follow-Up

Closed the Firebase/Firestore gap from the first TIC1 checkpoint.

Completed in this checkpoint:

- Added a shared `tenant_context_for_database(...)` adapter helper that turns a
  Firestore database project ID into an application `TenantIsolationContext`.
- Updated Firestore operation helpers so commit, batch write, batch get,
  begin/rollback transaction, get document, list collection IDs, run query,
  and run aggregation query validate the database tenant against the context
  before calling `Service`.
- Updated Firebase REST routes to construct the context from the decoded route
  database and request principal before provider-facing operation calls.
- Updated Firestore unary gRPC routes to construct the same context before
  provider-facing operation calls.
- Updated Firestore Listen and Write streaming setup so subscription and write
  execution use a context derived from the stream database and authenticated
  principal.
- Added tests proving mismatched Firestore database tenants are rejected before
  service/provider access and that valid database projects produce the expected
  tenant context.

Verification evidence:

- `cargo check -p nimbus-server`
  - result: pass; `Finished dev profile` in 3.70s
- `cargo test -p nimbus-server firestore_database_context -- --nocapture`
  - result: pass; 2 passed, 0 failed, 703 filtered out
- `cargo test -p nimbus-server firebase -- --nocapture`
  - result: pass; 134 passed, 0 failed, 571 filtered out
- `cargo fmt --all --check`
  - result: pass
- `git diff --check -- crates/nimbus-server/src docs/plans/tenant-isolation-control-plane-plan.md`
  - result: pass

Remaining before TIC1 is done:

- Tenant membership/role policy is still not a global access decision. TIC5
  must close that as authorization, not only an identity-carrier shape.
- Runtime grant and execution-tier admission is not complete. TIC4 still needs
  production-mode rejection for unsafe in-process grants and per-tenant budget
  enforcement beyond existing invocation counters.
- The context currently carries tenant, authority, and surface. Later TIC
  phases should extend or wrap it with admitted deployment, runtime, network,
  storage, volume, image, secret, and quota policy instead of adding parallel
  ad hoc arguments.
- Dirty worktree caveat: this checkpoint is mixed into an already-dirty
  workspace that includes unrelated generated Convex/demo artifacts,
  `package-lock.json`, desktop-auth proof images/plans, and prior docs edits.

### 2026-05-22 TIC1 Runtime Bundle Identity Follow-Up

Added a runtime identity gate to the tenant context baseline.

Completed in this checkpoint:

- Added `TenantIsolationContext::ensure_runtime_bundle_matches(...)` so a
  tenant-labelled `RuntimeBundleIdentity` must match the server-owned
  invocation tenant before runtime execution.
- Updated Convex top-level runtime invocation and nested runtime dispatch to
  validate runtime bundle tenant identity before invoking V8.
- Updated Cloud Functions HTTP and trigger execution to validate runtime bundle
  tenant identity before invoking V8.
- Added a direct TIC1 test proving mismatched runtime bundle tenant labels are
  rejected before invocation.

Verification evidence:

- `cargo check -p nimbus-server`
  - result: pass; `Finished dev profile` in 5.67s
- `cargo test -p nimbus-server tenant_isolation -- --nocapture`
  - result: pass; 4 passed, 0 failed, 702 filtered out
- `cargo test -p nimbus-server cloud_functions -- --nocapture`
  - result: pass; 38 passed, 0 failed, 668 filtered out
- `cargo test -p nimbus-server convex -- --nocapture`
  - result: pass; 116 passed, 0 failed, 5 ignored, 585 filtered out
  - `tests/reactive_loop.rs`: 18 passed, 0 failed, 14 filtered out
- `cargo fmt --all --check`
  - result: pass
- `git diff --check -- crates/nimbus-server/src docs/plans/tenant-isolation-control-plane-plan.md`
  - result: pass

Remaining before TIC1 is done:

- Closed by the next checkpoint: deployment generation was not yet carried in
  the context when this checkpoint landed.
- Tenant membership/role policy remains a TIC5 authorization task.
- The context still needs later TIC phases to attach admitted network, storage,
  volume, image, secret, and quota policy rather than passing those as
  independent arguments.

### 2026-05-22 TIC1 Deployment Generation Follow-Up

Added active deployment-generation binding to the tenant context baseline.

Completed in this checkpoint:

- Added optional deployment generation to `TenantIsolationContext`.
- Added `ensure_deployment_generation_matches(...)` so stale or mismatched
  deployment identities can be rejected before runtime execution.
- Bound Convex application routes and Convex HTTP actions to the active
  deployment generation when they create their tenant context.
- Bound Cloud Functions HTTP/callable execution to the active deployment
  generation and installed trigger executors with the generation they were
  activated under.
- Added a direct TIC1 test proving mismatched deployment generations are
  rejected before runtime invocation.

Verification evidence:

- `cargo check -p nimbus-server`
  - result: pass; `Finished dev profile` in 5.53s
- `cargo test -p nimbus-server tenant_isolation -- --nocapture`
  - result: pass; 5 passed, 0 failed, 702 filtered out
- `cargo test -p nimbus-server cloud_functions -- --nocapture`
  - result: pass; 38 passed, 0 failed, 669 filtered out
- `cargo test -p nimbus-server convex -- --nocapture`
  - result: pass; 116 passed, 0 failed, 5 ignored, 586 filtered out
  - `tests/reactive_loop.rs`: 18 passed, 0 failed, 14 filtered out
- `cargo fmt --all --check`
  - result: pass
- `git diff --check -- crates/nimbus-server/src docs/plans/tenant-isolation-control-plane-plan.md`
  - result: pass

Remaining before TIC1 is done:

- MongoDB compatibility commands still derive tenant authority directly from
  Mongo database names before service calls. Route them through the same
  context/admission artifact or explicitly document why MongoDB is outside the
  production tenant-isolation surface.
- Convex runtime subscription re-evaluation reconstructs a tenant context from
  an already-admitted tenant ID but does not yet carry the original deployment
  generation through the retained subscription transform.
- Tenant membership/role policy remains a TIC5 authorization task.

### 2026-05-22 TIC1 MongoDB And Subscription Closure

Closed the remaining known TIC1 gaps and moved TIC1 to `done`.

Completed in this checkpoint:

- Added a MongoDB compatibility tenant-context helper that maps Mongo database
  names to a `TenantIsolationContext`, keeps Mongo internal databases on the
  default tenant, and rejects context/database tenant mismatches before engine
  access.
- Routed MongoDB CRUD, collection, index, aggregation, change-stream, and
  transaction-session paths through the context helper before service calls.
  Transaction sessions now retain the admitted tenant context instead of only a
  free tenant ID.
- Added `TenantIsolationContext::reauthorize_application(...)` so a
  generation-aware application context can preserve tenant and deployment
  identity while changing the current application principal/surface.
- Carried the Convex WebSocket route context through the subscription socket
  session, runtime subscription bootstrap, and retained subscription
  re-evaluation forwarder so subscription runtime re-evaluation keeps the
  active deployment generation.
- Tightened Cloud Functions HTTP and trigger runtime service snapshots so
  service grants are read from the tenant isolation context before the runtime
  invocation request is built.
- Ran a focused seam audit over tenant parsing, service launch, runtime
  invocation context construction, service snapshots, and storage-facing
  engine calls. The remaining cross-tenant membership/role and `_nimbus`
  control-plane authorization work belongs to TIC5; runtime grant/tier policy
  belongs to TIC4.

Verification evidence:

- `cargo check -p nimbus-server`
  - result: pass; `Finished dev profile` in 3.88s
- `cargo test -p nimbus-server tenant_isolation -- --nocapture`
  - result: pass; 6 passed, 0 failed, 705 filtered out
- `cargo test -p nimbus-server mongodb -- --nocapture`
  - result: pass; 266 passed, 0 failed, 0 ignored, 444 filtered out
  - `tests/mongodb_spec/main.rs`: 0 matching tests, 23 filtered out
  - `tests/reactive_loop.rs`: 0 matching tests, 32 filtered out
- `cargo test -p nimbus-server convex -- --nocapture`
  - result: pass; 116 passed, 0 failed, 5 ignored, 590 filtered out
  - `tests/reactive_loop.rs`: 18 passed, 0 failed, 14 filtered out
- `cargo test -p nimbus-server cloud_functions -- --nocapture`
  - result: pass; 38 passed, 0 failed, 673 filtered out
- `cargo fmt --all --check`
  - result: pass
- `git diff --check -- crates/nimbus-server/src docs/plans/tenant-isolation-control-plane-plan.md`
  - result: pass

Remaining after TIC1:

- TIC2 must tenant-scope sandbox mutable artifacts and cleanup.
- TIC3 must fail-close network admission and port ownership.
- TIC4 must reject unsafe production in-process runtime tier/backend/grant
  combinations and enforce per-tenant runtime budgets.
- TIC5 must make tenant membership/role authorization global across native
  HTTP, adapters, scheduler, runtime host ops, WebSocket, and `_nimbus`
  control-plane surfaces.
- TIC6-TIC8 must close volumes/images/secrets/mounts, microVM quotas, and the
  two-tenant end-to-end proof harness.
- Dirty worktree caveat: this checkpoint remains mixed into an already-dirty
  workspace that includes unrelated generated Convex/demo artifacts,
  `package-lock.json`, desktop-auth proof images/plans, and prior docs edits.

### 2026-05-22 TIC2 Tenant-Owned Sandbox Artifact Roots

Closed the existing sandbox filesystem-state portion of TIC2 for krun and
container backends.

Completed in this checkpoint:

- Added a shared `nimbus-sandbox` artifact path module that roots bundle,
  state, manifest, conmon log, exit/persist, and materialized rootfs paths by
  tenant before sandbox ID:
  `tenants/<tenant_id>/sandboxes/<sandbox_id>/...`.
- Updated krun and container start planning so OCI bundles, conmon state,
  manifests, logs, and materialized image/build rootfs directories use the
  tenant-owned path layout before any `config.json` or rootfs is written.
- Kept the OCI blob cache shared only as a top-level content-addressed cache;
  tenant cleanup leaves it in place while removing tenant-owned mutable roots.
- Updated state inspection and port scans to enumerate the tenant-rooted
  manifest layout instead of the former global `containers/<sandbox_id>` path.
  Ambiguous duplicate sandbox IDs across tenant roots now fail instead of
  silently selecting a cross-tenant manifest.
- Added `SandboxBackend::remove_tenant_artifacts(...)` and wired
  `SandboxServiceManager::teardown_tenant(...)` to stop tracked services,
  project stopped service/port state, and then remove only the target tenant's
  sandbox artifact roots.
- Updated Linux krun smoke path expectations so real-host verification follows
  the tenant-rooted bundle, manifest, and log layout.

Verification evidence:

- `cargo test -p nimbus-sandbox -- --nocapture`
  - result: pass; 98 passed, 0 failed, 0 ignored in `src/lib.rs`; 2 passed,
    0 failed in `src/bin/neovex-guest-user-switch.rs`; Linux smoke target and
    doc tests had 0 runnable tests on this macOS host.
- `cargo test -p nimbus-server service_manager -- --nocapture`
  - result: pass; 7 passed, 0 failed, 704 filtered out; MongoDB spec and
    reactive-loop integration targets had 0 matching tests.
- `cargo test -p nimbus-server delete_tenant_stops_manager_owned_sandbox_services -- --nocapture`
  - result: pass; 1 passed, 0 failed, 710 filtered out; MongoDB spec and
    reactive-loop integration targets had 0 matching tests.

Remaining after TIC2:

- TIC3 must tenant-own network exposure policy, port leases, and container
  network/IPAM cleanup evidence. This checkpoint did not claim non-loopback or
  runtime-localhost isolation.
- TIC6 must add named volume admission and paths. Current service bundles still
  do not lower tenant-provided volume mounts; the only krun bind mount remains
  the Nimbus-owned read-only guest-user helper mount.
- TIC4/TIC5/TIC7/TIC8 remain open as recorded in the phase ledger.
- Dirty worktree caveat: this checkpoint remains mixed into an already-dirty
  workspace that includes unrelated generated Convex/demo artifacts,
  `package-lock.json`, desktop-auth proof images/plans, and prior docs edits.

### 2026-05-22 TIC3 Fail-Closed Compose Exposure Baseline

Started TIC3 at the service-declaration network admission seam.

Completed in this checkpoint:

- Verified that default Compose `HOST:CONTAINER` mappings already lower to
  `127.0.0.1:HOST:CONTAINER`.
- Changed Compose `HOST_IP:HOST:CONTAINER` lowering to reject non-loopback host
  addresses until Nimbus has an explicit operator network exposure policy
  object.
- Added regression coverage for `0.0.0.0:HOST:CONTAINER` failing closed with
  operator-policy guidance.
- Added explicit `tenantId`, `serviceName`, and `endpointName` fields to
  system `ports` documents so projected port ownership is machine-readable
  instead of only embedded in `serviceId`.

Verification evidence:

- `cargo test -p nimbus-bin compose_project_rejects_non_loopback_port_exposure_without_policy -- --nocapture`
  - result: pass; 1 passed, 0 failed, 512 filtered out; `server_discovery_serde`
    target had 0 matching tests.
- `cargo test -p nimbus-bin compose_project_lowers_into_sandbox_service_catalog -- --nocapture`
  - result: pass; 1 passed, 0 failed, 512 filtered out; `server_discovery_serde`
    target had 0 matching tests.
- `cargo test -p nimbus-server service_manager -- --nocapture`
  - result: pass; 7 passed, 0 failed, 704 filtered out; MongoDB spec and
    reactive-loop integration targets had 0 matching tests.
- `cargo fmt --all --check`
  - result: pass
- `git diff --check -- crates/nimbus-bin/src/compose/file/parse.rs crates/nimbus-bin/src/compose/file/tests.rs crates/nimbus-server/src/system_tenant.rs crates/nimbus-server/src/service_manager.rs`
  - result: pass

Remaining before TIC3 is done:

- Port leases need per-tenant quota enforcement and cleanup evidence beyond
  system-state ownership projection.
- The Linux krun/libkrun localhost-only TSI proof must be rerun or retained as
  evidence after the remaining TIC3 changes.
- Dirty worktree caveat: this checkpoint remains mixed into an already-dirty
  workspace that includes unrelated generated Convex/demo artifacts,
  `package-lock.json`, desktop-auth proof images/plans, and prior docs edits.

### 2026-05-22 TIC3/TIC4 Production Runtime Network Admission

Closed the highest-risk runtime-network bypass from TIC3 by adding a
production runtime admission gate at the server-owned tenant isolation seam.

Completed in this checkpoint:

- Added public `TenantIsolationMode::{LocalDevelopment, Production}` to server
  options and router state. Local development remains the default so measured
  Node compatibility tests keep their localhost grants.
- Added `TenantIsolationContext::ensure_runtime_policy_admitted(...)` for the
  `in_process_untrusted` tier. In production it rejects generic localhost or
  wildcard `net_connect`, any `net_listen`, `run`, `ffi`, `env_write`,
  `identity`, `tool`, `worker`, `inspector`, `Privileged`, non-application
  presets, and broad filesystem/package-loading roots before runtime
  invocation.
- Wired the gate through Convex runtime query/mutation/action execution,
  runtime-backed subscriptions and re-evaluation, Cloud Functions HTTP, and
  Cloud Functions triggers.
- Added route-level proof that a Convex Node runtime function in production is
  rejected with `production in_process_untrusted` and `generic localhost`
  before its JavaScript entrypoint can run.

Verification evidence:

- `cargo test -p nimbus-server tenant_isolation -- --nocapture`
  - result: pass; 9 passed, 0 failed, 705 filtered out; MongoDB spec and
    reactive-loop integration targets had 0 matching tests.
- `cargo test -p nimbus-server cloud_functions -- --nocapture`
  - result: pass; 38 passed, 0 failed, 676 filtered out; MongoDB spec and
    reactive-loop integration targets had 0 matching tests.
- `cargo test -p nimbus-server convex_runtime_query_resolves_missing_service_bindings_via_services_get -- --nocapture`
  - result: pass; 1 passed, 0 failed, 713 filtered out; MongoDB spec and
    reactive-loop integration targets had 0 matching tests.
- `cargo test -p nimbus-server production_convex_node_runtime_rejects_loopback_network_grants_before_invocation -- --nocapture`
  - result: pass; 1 passed, 0 failed, 714 filtered out; MongoDB spec and
    reactive-loop integration targets had 0 matching tests.

Remaining before TIC3 is done:

- The Linux krun/libkrun localhost-only TSI proof must be rerun or retained as
  evidence after the remaining TIC3 changes.

Remaining before TIC4 is done:

- Production mode needs to become the explicit default for real multi-tenant
  deploy/serve entrypoints, while local development remains opt-in or
  dev-only.
- Rejected in-process workloads need a canonical trusted-only or microVM
  routing path instead of only a rejection.
- Runtime CPU, memory, execution-time, worker-thread, and nested-call budgets
  need tenant accounting beyond the current top-level concurrency/fairness
  limits.
- Native-addon and package-manager containment needs proof beyond the current
  grant/root-shape rejection.

### 2026-05-22 TIC3 Tenant Port Quota And Cleanup Evidence

Closed the port-ownership quota gap in TIC3 at the sandbox port materialization
seam shared by the krun and container backends.

Completed in this checkpoint:

- Added tenant-scoped manifest enumeration so quota accounting can count only
  the target tenant's active/not-ready/stopping port leases while global host
  port allocation still reserves ports already held by other tenants.
- Changed OCI port allocation to require a tenant ID before explicit or
  image-exposed ports are materialized.
- Added a default per-tenant published-port quota of 128 to both
  `KrunSandboxBackendConfig` and `ContainerSandboxBackendConfig`, with
  backend-specific overrides for tests or future operator policy.
- Ensured explicit `SandboxPortBinding` values count against the same quota as
  image-exposed auto ports, so predeclared ports cannot bypass admission.
- Kept stopped manifests out of quota and host-port reservation, preserving
  existing port cleanup/reuse behavior.
- Re-ran service-manager projection tests proving port records carry tenant,
  service, and endpoint identity on start and are removed on stop.

Verification evidence:

- `cargo test -p nimbus-sandbox port_manager -- --nocapture`
  - result: pass; 5 passed, 0 failed, 97 filtered out in `src/lib.rs`;
    guest-user-switch, Linux smoke, and doc-test targets had 0 matching tests.
- `cargo test -p nimbus-sandbox plan_only_backend_rejects_same_tenant_port_quota_for_image_exposed_ports -- --nocapture`
  - result: pass; 1 passed, 0 failed, 101 filtered out in `src/lib.rs`;
    guest-user-switch and Linux smoke targets had 0 matching tests.
- `cargo test -p nimbus-sandbox start_from_image_plan_only_rejects_same_tenant_port_quota_exhaustion -- --nocapture`
  - result: pass; 1 passed, 0 failed, 101 filtered out in `src/lib.rs`;
    guest-user-switch and Linux smoke targets had 0 matching tests.
- `cargo test -p nimbus-sandbox start_from_image_plan_only_auto_assigns_exposed_ports_and_reuses_released_ports -- --nocapture`
  - result: pass; 1 passed, 0 failed, 101 filtered out in `src/lib.rs`;
    guest-user-switch and Linux smoke targets had 0 matching tests.
- `cargo test -p nimbus-sandbox plan_only_backend_auto_assigns_exposed_ports_from_published_range -- --nocapture`
  - result: pass; 1 passed, 0 failed, 101 filtered out in `src/lib.rs`;
    guest-user-switch and Linux smoke targets had 0 matching tests.
- `cargo test -p nimbus-server ensure_service_binding_async_records_system_tenant_service_state -- --nocapture`
  - result: pass; 1 passed, 0 failed, 714 filtered out in `src/lib.rs`;
    MongoDB spec and reactive-loop integration targets had 0 matching tests.
- `cargo test -p nimbus-server local_admin_service_lifecycle_routes_start_stop_and_project_system_state -- --nocapture`
  - result: pass; 1 passed, 0 failed, 714 filtered out in `src/lib.rs`;
    MongoDB spec and reactive-loop integration targets had 0 matching tests.
- `cargo test -p nimbus-sandbox -- --nocapture`
  - result: pass; 102 passed, 0 failed, 0 ignored in `src/lib.rs`; 2 passed,
    0 failed in `src/bin/neovex-guest-user-switch.rs`; Linux smoke target and
    doc tests had 0 runnable tests on this macOS host.
- `cargo fmt --all --check`
  - result: pass

Remaining before TIC3 is done:

- Container backend network namespace and IPAM state must move under
  tenant-owned artifact roots and prove same sandbox/service names across
  tenants cannot share mutable network state.

### 2026-05-22 TIC3 Tenant-Owned Container Network State

Closed the final TIC3 mutable-network-state gap at the container backend's OCI
network layout seam.

Completed in this checkpoint:

- Changed `OciNetworkLayout` to require the admitted tenant ID before deriving
  network namespace, netavark status, and IPAM state paths.
- Rooted container network mutable state under
  `state/tenants/<tenant_id>/networks/...` instead of the backend-wide state
  root.
- Passed the resolved sandbox spec tenant from the container backend before
  bundle, network, or manifest materialization.
- Proved identical sandbox IDs across different tenants get distinct network
  namespace, status, and IPAM paths.
- Proved per-tenant IPAM state can allocate the same tenant-local container IP
  independently without sharing mutable state across tenants.
- Moved TIC3 to `done`; future tenant-owned endpoint-grant policy remains a
  TIC4/TIC8 capability-shaping concern, not an open default-exposure gap.

Verification evidence:

- `cargo test -p nimbus-sandbox network -- --nocapture`
  - result: pass; 14 passed, 0 failed, 90 filtered out in `src/lib.rs`;
    guest-user-switch, Linux smoke, and doc-test targets had 0 matching tests.
- `cargo test -p nimbus-sandbox plan_only_backend_scopes_network_state_by_tenant_for_same_sandbox_id -- --nocapture`
  - result: pass; 1 passed, 0 failed, 104 filtered out in `src/lib.rs`;
    guest-user-switch and Linux smoke targets had 0 matching tests.
- `cargo test -p nimbus-sandbox -- --nocapture`
  - result: pass; 105 passed, 0 failed, 0 ignored in `src/lib.rs`; 2 passed,
    0 failed in `src/bin/neovex-guest-user-switch.rs`; Linux smoke target and
    doc tests had 0 runnable tests on this macOS host.

Remaining after TIC3:

- TIC4 still needs production-mode defaulting/routing and tenant-accounted
  runtime CPU, memory, execution-time, worker-thread, and nested-call budgets.
- TIC5 must add storage/API principal authorization before provider access.
- TIC6 must add tenant-scoped volumes, images, secrets, and mounts.
- TIC7 must add broader microVM/resource quotas beyond the TIC3 port quota.
- TIC8 must add the two-tenant end-to-end proof harness.
- Rootless minicloud libkrun execution still has a separate diagnostic gap;
  the rootful Linux service path remains the TIC3 production proof.

### 2026-05-22 TIC4 Start Runtime Isolation Defaults And Budgets

Started TIC4 at the final CLI server entrypoint and runtime budget control
plane.

Completed in this checkpoint:

- Made `nimbus start` carry `TenantIsolationMode::Production` by default
  through `StartCommand` into `ServeOptions`, so production runtime admission is
  active for the normal foreground server path.
- Made `nimbus dev` explicitly set `TenantIsolationMode::LocalDevelopment`,
  preserving Node-compatible localhost grants only for the local-development
  command path.
- Added the selected tenant-isolation mode to the startup summary, making the
  production/local-development boundary visible in operator output.
- Added `--runtime-max-active-per-tenant`,
  `--runtime-max-in-flight-per-tenant`, and
  `--runtime-max-queued-per-tenant` flags, lowering them into
  `RuntimeLimits` so the existing per-tenant runtime admission counters are
  operator-configurable instead of only derived from global worker limits.
- Left `TenantIsolationMode::default()` unchanged for now so existing embedder
  APIs remain explicit follow-up work instead of a hidden behavior flip across
  every test and embedding surface.

Verification evidence:

- `cargo fmt --all --check`
  - result: pass
- `cargo test -p nimbus-bin cli_surface -- --nocapture`
  - result: pass; 25 passed, 0 failed, 491 filtered out in `src/main.rs`;
    `server_discovery_serde` had 0 matching tests.
- `cargo test -p nimbus-bin dev_plan_uses_project_local_persistence_root -- --nocapture`
  - result: pass; 1 passed, 0 failed, 515 filtered out in `src/main.rs`;
    `server_discovery_serde` had 0 matching tests.
- `cargo test -p nimbus-server tenant_isolation -- --nocapture`
  - result: pass; 9 passed, 0 failed, 706 filtered out in `src/lib.rs`;
    MongoDB spec and reactive-loop integration targets had 0 matching tests.

Remaining before TIC4 is done:

- Decide and document whether public library `serve*` defaults should flip to
  production isolation before launch or require an explicit production builder.
- Lower rejected `in_process_untrusted` workloads through a real
  `microvm_service` or trusted-only activation path instead of only returning
  an admission error with the canonical fallback tier.
- Add tenant-accounted CPU, memory, execution-time, worker-thread, and nested
  runtime budgets beyond the top-level active/in-flight/queued counters.
- Prove native-addon and package-manager containment beyond the current
  grant/root-shape rejection.

### 2026-05-22 TIC4 Runtime Rejection Routing Vocabulary

Converted production in-process runtime rejection from a flat error into an
admission result with a canonical fallback tier.

Completed in this checkpoint:

- Expanded the internal `RuntimeIsolationTier` vocabulary to include
  `in_process_trusted_only`, `microvm_service`, and future
  `wasm_capability_sandbox` alongside the current
  `in_process_untrusted` gate.
- Classified production `in_process_untrusted` rejection reasons by fallback:
  trusted/operator-like grants route to `in_process_trusted_only`, broad OS or
  network capabilities route to `microvm_service`, and non-JavaScript content
  routes to the future WASM capability tier.
- Updated production admission errors to include `route via <tier>`, giving
  deploy/activation code a stable control-plane vocabulary for the next
  routing slice.
- Kept validation scoped to `in_process_untrusted`; `microvm_service` and
  trusted-only tiers are not rejected by the in-process untrusted grant subset
  checker.
- Updated `docs/architecture/runtime/permission-model.md` so the architecture
  doc no longer says the tier enum is only future work.

Verification evidence:

- `cargo test -p nimbus-server tenant_isolation -- --nocapture`
  - result: pass; 11 passed, 0 failed, 706 filtered out in `src/lib.rs`;
    MongoDB spec and reactive-loop integration targets had 0 matching tests.
- `cargo fmt --all --check`
  - result: pass

Remaining before TIC4 is done:

- Lower `route via microvm_service` or `route via in_process_trusted_only`
  outcomes into actual deployment/activation behavior where Nimbus has enough
  service metadata to do so.
- Add tenant-accounted CPU, memory, execution-time, worker-thread, and nested
  runtime budgets beyond the top-level active/in-flight/queued counters.
- Prove native-addon and package-manager containment beyond the current
  grant/root-shape rejection.

### 2026-05-22 TIC4 Public Serve Production Default

Closed the public serve/default contract decision for TIC4.

Completed in this checkpoint:

- Flipped `TenantIsolationMode::default()` to `Production`, which makes
  `ServeOptions::default()`, public `serve*` helpers, and router builders enter
  production tenant-isolation mode unless the caller explicitly overrides it.
- Kept `nimbus dev` on `LocalDevelopment` through the already-explicit
  `StartCommand` field, so local Node compatibility remains a conscious dev
  choice rather than the server default.
- Added a regression test proving the isolation mode default is production.

Verification evidence:

- `cargo test -p nimbus-server tenant_isolation -- --nocapture`
  - result: pass; 12 passed, 0 failed, 706 filtered out in `src/lib.rs`;
    MongoDB spec and reactive-loop integration targets had 0 matching tests.
- `cargo test -p nimbus-bin cli_surface -- --nocapture`
  - result: pass; 25 passed, 0 failed, 491 filtered out in `src/main.rs`;
    `server_discovery_serde` had 0 matching tests.
- `cargo test -p nimbus-bin dev_plan_uses_project_local_persistence_root -- --nocapture`
  - result: pass; 1 passed, 0 failed, 515 filtered out in `src/main.rs`;
    `server_discovery_serde` had 0 matching tests.
- `cargo fmt --all --check`
  - result: pass

Remaining before TIC4 is done:

- Lower `route via microvm_service` or `route via in_process_trusted_only`
  outcomes into actual deployment/activation behavior where Nimbus has enough
  service metadata to do so.
- Add tenant-accounted CPU, memory, execution-time, worker-thread, and nested
  runtime budgets beyond the top-level active/in-flight/queued counters.
- Prove native-addon and package-manager containment beyond the current
  grant/root-shape rejection.

Remaining after this checkpoint:

- TIC4 still needs production runtime fallback routing and tenant-accounted
  runtime CPU, memory, execution-time, worker-thread, and nested-call budgets.
- TIC5-TIC8 remain open as recorded in the phase ledger.
- Dirty worktree caveat: this checkpoint remains mixed into an already-dirty
  workspace that includes unrelated generated Convex/demo artifacts,
  `package-lock.json`, desktop-auth proof images/plans, and prior docs edits.

### 2026-05-22 TIC5 Application Tenant-Claim Gate

Started TIC5 with a conservative adapter-boundary tenant-claim gate for
application principals.

Completed in this checkpoint:

- Added `TenantIsolationContext::ensure_application_principal_tenant_access(...)`
  as the shared server-owned gate for checking application principals against
  the admitted tenant before tenant-facing provider/runtime work.
- The gate prefers `verified_claims` over identity claims and recognizes
  `tenant_id`, `tenantId`, `nimbus_tenant_id`, and `nimbusTenantId` when they
  are string claims.
- Wired the gate through Convex function routes, Convex HTTP actions, Cloud
  Functions HTTP runtime dispatch, and Firebase/Firestore database context
  creation before those paths reach storage or runtime execution.
- Proved a principal scoped to tenant A is rejected when it targets tenant B,
  while anonymous/public app requests with no tenant claim remain allowed by
  design until Nimbus has a full tenant-membership/session model.

Verification evidence:

- `cargo fmt --all --check`
  - result: pass
- `cargo test -p nimbus-server tenant_isolation -- --nocapture`
  - result: pass; 14 passed, 0 failed, 707 filtered out in `src/lib.rs`;
    MongoDB spec and reactive-loop integration targets had 0 matching tests.
- `cargo test -p nimbus-server firestore_database_context -- --nocapture`
  - result: pass; 3 passed, 0 failed, 718 filtered out in `src/lib.rs`;
    MongoDB spec and reactive-loop integration targets had 0 matching tests.

Remaining before TIC5 is done:

- Native HTTP/local operator routes need a richer scoped session and tenant
  membership model beyond today's local-admin operator authority.
- Convex HTTP actions, Firebase/Firestore, and Cloud Functions need
  end-to-end swapped-tenant HTTP tests with a real or fake verifier so the
  transport status codes and response envelopes are covered, not only the
  shared gate.
- Scheduler and runtime HostBridge paths need proof that every provider call
  uses the server-owned invocation tenant rather than any tenant value supplied
  by JavaScript payloads.
- `_nimbus` visibility needs explicit operator-only proof across tenant-facing
  adapters.

### 2026-05-22 TIC5 Convex Swapped-Tenant HTTP Proof

Added the first transport-level proof that the application tenant-claim gate
fires through a real adapter route and HTTP error envelope.

Completed in this checkpoint:

- Added a local-server security test that configures the real Convex custom-JWT
  verifier, creates `tenant-a` and `tenant-b`, and issues a signed token whose
  verified custom claim is `tenant_id=tenant-b`.
- Proved `/convex/tenant-b/query` succeeds with that token while
  `/convex/tenant-a/query` returns `403 Forbidden` before the query can run.
- Asserted the rejected response names both the authorized claim tenant and the
  rejected path tenant, making swapped-tenant failures diagnosable.

Verification evidence:

- `cargo fmt --all --check`
  - result: pass
- `cargo test -p nimbus-server convex_route_rejects_application_bearer_for_different_tenant -- --nocapture`
  - result: pass; 1 passed, 0 failed, 721 filtered out in `src/lib.rs`;
    MongoDB spec and reactive-loop integration targets had 0 matching tests.
- `cargo test -p nimbus-server local_server_security -- --nocapture`
  - result: pass; 11 passed, 0 failed, 711 filtered out in `src/lib.rs`;
    MongoDB spec and reactive-loop integration targets had 0 matching tests.

Remaining before TIC5 is done:

- Convex HTTP actions still need a route-level swapped-tenant proof.
- Firebase/Firestore gRPC/listen and Cloud Functions HTTP need equivalent
  transport-level swapped-tenant proofs.
- WebSocket/subscription auth needs an explicit scoped-session or bearer
  renewal proof before it can be marked covered.
- Native HTTP/local operator routes need the scoped session and tenant
  membership model noted above.
- Scheduler/runtime HostBridge and `_nimbus` visibility proof remain open.

### 2026-05-22 TIC5 Firebase REST Swapped-Tenant Proof

Added the Firestore REST counterpart to the Convex swapped-tenant HTTP proof.

Completed in this checkpoint:

- Added a Firebase auth/availability test that configures the real application
  custom-JWT verifier, creates `tenant-a` and `tenant-b`, and issues a signed
  token whose verified custom claim is `tenant_id=tenant-b`.
- Proved `/v1/projects/tenant-b/databases/(default)/documents:batchGet`
  succeeds with that token while the same request shape targeting
  `/v1/projects/tenant-a/databases/(default)/documents:batchGet` returns
  `403 Forbidden`.
- Asserted the Firebase error response names both the authorized claim tenant
  and the rejected Firestore project tenant.

Verification evidence:

- `cargo test -p nimbus-server firebase_rest_batch_get_rejects_application_bearer_for_different_tenant -- --nocapture`
  - result: pass; 1 passed, 0 failed, 722 filtered out in `src/lib.rs`;
    MongoDB spec and reactive-loop integration targets had 0 matching tests.
- `cargo test -p nimbus-server firebase_auth_and_availability -- --nocapture`
  - result: pass; 10 passed, 0 failed, 713 filtered out in `src/lib.rs`;
    MongoDB spec and reactive-loop integration targets had 0 matching tests.
- `cargo fmt --all --check`
  - result: pass

Remaining before TIC5 is done:

- Convex HTTP actions still need a route-level swapped-tenant proof.
- Firebase/Firestore gRPC and listen/WebSocket auth need scoped-tenant proofs.
- Cloud Functions public unauthenticated HTTP exposure needs an explicit note or
  policy decision separate from callable auth, because it does not carry bearer
  principal authority today.
- Native HTTP/local operator routes need the scoped session and tenant
  membership model noted above.
- Scheduler/runtime HostBridge and `_nimbus` visibility proof remain open.

### 2026-05-22 TIC5 Cloud Functions Callable Swapped-Tenant Proof

Added the Cloud Functions callable counterpart to the Convex and Firebase REST
transport proofs.

Completed in this checkpoint:

- Added a callable test with a fake application-auth verifier that returns
  `InvocationAuth` carrying a verified `tenant_id` claim from the bearer token.
- Proved a callable request with `Bearer tenant:tenant-a` succeeds against the
  implicit single-tenant Cloud Functions binding for `tenant-a`.
- Proved the same callable target rejects `Bearer tenant:tenant-b` with
  `403 Forbidden` / Firebase callable `PERMISSION_DENIED`, and that the error
  names both the authorized claim tenant and the implicit target tenant.

Verification evidence:

- `cargo test -p nimbus-server cloud_functions_callable_rejects_application_bearer_for_different_tenant -- --nocapture`
  - result: pass; 1 passed, 0 failed, 723 filtered out in `src/lib.rs`;
    MongoDB spec and reactive-loop integration targets had 0 matching tests.
- `cargo test -p nimbus-server cloud_functions_callable -- --nocapture`
  - result: pass; 5 passed, 0 failed, 719 filtered out in `src/lib.rs`;
    MongoDB spec and reactive-loop integration targets had 0 matching tests.
- `cargo fmt --all --check`
  - result: pass

Remaining before TIC5 is done:

- Convex HTTP actions still need a route-level swapped-tenant proof.
- Firebase/Firestore gRPC and listen/WebSocket auth need scoped-tenant proofs.
- Cloud Functions public unauthenticated HTTP exposure needs an explicit note or
  policy decision separate from callable auth, because it does not carry bearer
  principal authority today.
- Native HTTP/local operator routes need the scoped session and tenant
  membership model noted above.
- Scheduler/runtime HostBridge and `_nimbus` visibility proof remain open.

### 2026-05-22 TIC3 Rootful Linux Localhost-Only Proof

Reran the krun/libkrun localhost-only smoke after the port quota changes on
the Debian 13 `minicloud` Linux host.

Completed in this checkpoint:

- Synced the committed Nimbus source snapshot from local `main` commit
  `f5acd4aa` to `~/src/github.com/nimbus/nimbus` on `minicloud` for proof
  execution. The remote source tree is a copied checkout, not a git worktree.
- Confirmed host readiness with KVM, buildah, conmon, private
  `/usr/libexec/nimbus/crun`, private `/usr/libexec/nimbus/lib/libkrun.so.1`,
  private `/usr/libexec/nimbus/lib/libkrunfw.so.5`, and exported
  `krun_set_port_map_with_bind_address`.
- Installed a per-user rustup stable toolchain for the `nimbus` user because
  Debian 13 currently provides `rustc 1.85.0`, while the current Nimbus
  lockfile requires at least `rustc 1.86` through ICU dependencies.
- Ran the ignored Linux smoke under `sudo` with an isolated target directory so
  root-owned build artifacts stay outside the repo. The test booted
  BusyBox through the private crun/libkrun stack, reached Ready, proved
  loopback HTTP, and proved the same port refused the concrete non-loopback
  address `192.168.4.29`.

Verification evidence:

- `ssh nimbus@192.168.4.29 'cd ~/src/github.com/nimbus/nimbus && bash scripts/check-vmm-host.sh'`
  - result: pass; `result supported`; `/dev/kvm access=ok`;
    private crun `1.27.1-dirty` reports `+LIBKRUN`; private libkrun release
    `v1.17.4-nimbus.1`; `krun_set_port_map_with_bind_address` present;
    private crun runpath has `$ORIGIN/lib`.
- `ssh nimbus@192.168.4.29 'cd ~/src/github.com/nimbus/nimbus && sudo env PATH=/home/nimbus/.cargo/bin:... CARGO_HOME=/home/nimbus/.cargo CARGO_TARGET_DIR=/tmp/nimbus-root-cargo-target NIMBUS_KRUN_SMOKE_WORKDIR=/tmp/nimbus-tic3-root-krun-smoke NIMBUS_KRUN_SMOKE_RUNTIME=/usr/libexec/nimbus/crun NIMBUS_KRUN_SMOKE_CONMON=/usr/bin/conmon NIMBUS_KRUN_SMOKE_BUILDAH=/usr/bin/buildah NIMBUS_KRUN_SMOKE_NON_LOOPBACK_HOST=192.168.4.29 cargo test -p nimbus-sandbox krun_backend_image_backed_smoke_pulls_and_boots_busybox --test krun_linux_smoke -- --ignored --nocapture'`
  - result: pass; 1 passed, 0 failed, 10 filtered out; output included
    `non-loopback bind probe 192.168.4.29:18081: Connection refused`.

Diagnostic caveat:

- The same smoke as non-root did not pass on `minicloud`: after rustup fixed
  compilation, the test timed out waiting for Ready and the exit file recorded
  `139`; `dmesg` showed `libkrun VM` general-protection faults. A direct
  rootless diagnostic also hit `/dev/kvm` permission denial from inside
  `buildah unshare`. The supported rootful service path passed, but rootless
  minicloud libkrun execution should be tracked separately from TIC3's
  production localhost-only proof.

Remaining before TIC3 is done:

- Container backend network namespace and IPAM state must move under
  tenant-owned artifact roots and prove same sandbox/service names across
  tenants cannot share mutable network state.

## Execution Notes

- Do not weaken the completed krun hardening proof. The patched
  `nimbus-crun`/`nimbus-libkrun` stack remains the baseline for service
  exposure.
- Do not add compatibility shims for unsafe Compose behavior. This repo is
  pre-launch; reject unsafe inputs directly.
- Keep tenant isolation independent from runtime engine selection. V8, Deno,
  Bun/JSC, and future WASM engines must all consume the same tenant isolation
  context and grants.
