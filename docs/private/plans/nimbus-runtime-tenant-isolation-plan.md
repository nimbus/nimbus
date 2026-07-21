# Nimbus Runtime Tenant Isolation Plan

Status: `proposed; launch-safety owner decision required before implementation`

Owner: this plan is the sole implementation owner for tenant ownership of
retained Nimbus runtime state, runtime-owner retirement, and the move from
adapter-owned executors to a compute-owned Nimbus runtime manager.

Provenance: source review and focused verification on 2026-07-21, prompted by
the isolate-density / `IsolateGroup` review, then reconciled against
`origin/main` at `93124d87e` after durable tenant incarnations landed in PR
#224. This is a correctness follow-on to the completed profile-aware isolate
runtime work. The completed PIR plan and its final architecture record remain
the source for profile selection, snapshots, code cache, scheduling, density,
and pointer compression; this plan owns reconciliation where the implementation
does not structurally enforce PIR's stated same-owner reuse invariant.

Priority: complete before Nimbus makes a multi-tenant launch claim. The
distinct-tenant default path is currently partitioned, but non-default routing
policies and same-ID tenant recreation can defeat the intended ownership model.
Cross-tenant mutable runtime state is not an acceptable residual risk.

## Outcome

Nimbus will have one runtime ownership model across V8, Wasmtime, Bun/JSC, and
future backends:

> Any runtime substrate that has executed guest code is irrevocably owned by
> one admitted runtime owner incarnation. Routing locality may improve cache
> hits, but it can never weaken that ownership. Reuse requires an exact owner,
> deployment, bundle, runtime-shape, and authority match. Revocation, deletion,
> ambiguity, or mismatch condemns the retained substrate.

Convex, Cloud Functions, and future protocol adapters use the Nimbus runtime.
They may select guest semantics, compatibility targets, and bundle artifacts;
they do not own runtime pooling, tenant isolation, runtime defaults, executor
lifecycle, or reuse authority.

The intended topology is shared infrastructure with structurally separate
owner partitions, not a thread/executor pool per tenant:

```text
Nimbus RuntimeManager (nimbus-compute)
  -> runtime lane (backend + profile + guest semantics)
     -> worker thread
        -> shareable bootstrap state
        -> owner partition: tenant A / incarnation 17
        -> owner partition: tenant B / incarnation 4
        -> condemned state awaiting thread-affine destruction
```

This preserves density while making tenant separation a property of the pool
interface rather than a convention followed by current callers.

## Terminology Decision

- **Nimbus runtime** is the execution substrate.
- A **runtime lane** is a Nimbus-owned backend/profile/guest-semantics lane.
- The Convex adapter and Cloud Functions adapter are callers of that module.
- `ConvexRuntimeLane` is an adapter-owned implementation detail today, not a
  separate runtime. This plan removes that ownership and adapter-centric name.
- **Routing affinity** means best-effort worker locality only.
- **Runtime owner ID** means an authority-bearing tuple of owner class, opaque
  stable principal/subject ID, and incarnation. The principal ID prevents two
  tenants whose per-tenant incarnation counters are both `1` from comparing
  equal. A human-readable tenant/audit label is metadata, not identity.
- **Runtime owner lease** means a manager-issued owner ID plus live revocation
  state for queued, checked-out, and returning work.
- **Reuse authority** means the complete exact-match facts required after the
  owner matches.
- **Isolate group** retains its V8 meaning: a pointer-compression cage and
  associated V8 shared resources. It is not a tenant owner or security domain.

Do not use “Convex runtime” in architecture or implementation prose except
when quoting an external Convex compatibility term. Use “the Nimbus runtime
lane used by the Convex adapter” where the adapter distinction matters.

Current code also uses local names such as `runtime_owner` for a
`NimbusRuntime` execution facade. RTI1 must rename that vocabulary to
`runtime`/`runtime_instance` while introducing authority-bearing runtime owners;
the same term must not mean both an executor facade and a tenant principal.

## Confirmed Current State

### Runtime topology

- `RuntimeExecutor` creates a fixed set of worker threads.
- Each V8 worker owns one `V8WorkerRuntimePool`.
- The V8 warm pool is a flat `Vec<WarmPoolEntry>` containing complete
  `JsRuntime` instances; it is not a pool object per tenant.
- Worker routing affinity is best effort. A busy affinity worker may spill to a
  less-loaded worker, so a tenant can legitimately have retained entries on
  more than one worker.
- Pool lookup uses exact equality on `RuntimePoolPartitionKey`.
- The current V8 key receives tenant identity only through
  `RuntimeAffinityKey` or a tenant-scoped bundle identity.

### Retained-state behavior

- `WarmPool` retains the evaluated `JsRuntime`. Module evaluation is skipped
  on a warm hit. Module globals, closures, singleton caches, and other
  tenant-influenced JavaScript heap state persist by contract.
- Return cleanup resets request/host state and enforces cleanliness, heap, and
  reuse limits. It does not erase module globals.
- `WarmContextRecycle` creates a fresh realm and module map but retains the
  outer isolate. Under the required invariant, that isolate remains owned by
  the first owner that executed guest code; fresh realm creation does not make
  it globally clean.
- Wasmtime `RetainedStorePool` retains a `Store`, resets invocation host state,
  and creates a new component instance. Its authority key contains an optional
  tenant label but no owner incarnation.
- Bun/JSC trusted retained mode is currently non-product-selectable scaffold.
  It does not yet retain a real linked VM, which makes this the right time to
  require the common owner contract before that implementation lands.

### Tenant lifecycle authority

- Engine/storage now allocate a positive durable tenant incarnation at tenant
  creation and preserve the counter outside the deleted tenant database.
  Provider-backed tenants return the same canonical fact from provider
  metadata. The value is monotonic per tenant, not globally unique.
- `TenantRuntime` already owns the tenant operation/delete gate: deletion
  rejects new Engine operations and drains admitted operations before removing
  persistence. That authority is not currently exposed as a compute/runtime
  owner lease, and a runtime invocation does not hold it for its full guest
  lifetime.
- The native HTTP handler still sequences runtime-service teardown and Engine
  deletion itself. Tenant lifecycle orchestration therefore remains in the
  transport layer instead of a compute-owned boundary that can also retire
  runtime queues and retained state.

### Shareable state

The following may remain shared when their existing exact compatibility and
provenance checks pass:

- platform-built V8 startup snapshots;
- the process-lifetime NodeFull read-only-heap anchor;
- V8 compiled-module code-cache bytes;
- Wasmtime compiled components and engine state;
- future pre-created runtimes that have never evaluated or entered guest code.

These are bootstrap or immutable derived artifacts, not retained tenant heaps.
An unassigned clean runtime may transition to one owner exactly once. It may
never transition back to globally clean after guest entry.

## Confirmed Findings

| ID | Finding | Reachability | Required correction |
| --- | --- | --- | --- |
| RTIF1 | `RuntimeRoutingAffinity::None` produces no affinity key, so two tenants with the same bundle and policy can produce the same V8 reuse key. | Source-confirmed. The CLI does not expose this efficiency knob, but the public/internal runtime policy interface does. | Make a mandatory owner key independent of routing affinity. `None` may disable locality, never ownership. |
| RTIF2 | `RuntimeRoutingAffinity::Script` derives its optional tenant from `RuntimeBundleIdentity`, not the invocation. Convex and Cloud Functions load deployment-wide unscoped bundles. | Source-confirmed. Two tenant invocations of an unscoped bundle can produce the same script-affinity reuse key. | Keep script identity as optional locality/reuse granularity only; always pair it with the mandatory owner. |
| RTIF3 | Native tenant deletion tears down runtime-backed named services and Engine tenant state but does not revoke runtime work, purge worker affinity, or evict retained runtimes. | Source-confirmed lifecycle gap. A delete followed by recreation of the same tenant ID can match a stale entry if the deployment and policy are unchanged. RTI0 must add the deterministic fail-before behavior test. | Carry the canonical owner incarnation into runtime work, add revocation, queued/in-flight handling, worker-wide retirement, and an exact same-ID recreation test. |
| RTIF4 | The current runtime owner surrogate is the tenant label. Engine/storage now have a durable per-tenant incarnation, but it is not carried into runtime admission or retained-state keys. Deployment generation and `TenantIsolationDecisionId` are not substitutes. | Source-confirmed on current `main`. | Build the tenant runtime-owner ID from the canonical tenant subject plus Engine/storage incarnation. Do not mint a parallel compute-local tenant incarnation or use tenant ID, decision ID, deployment generation, or the per-tenant counter alone. |
| RTIF5 | HostBridge state is rebound for each invocation, so host calls use the current admitted tenant. That does not clear JavaScript module globals or other guest-mutated memory. | Behavior and source-confirmed. | Preserve per-operation HostBridge checks and independently enforce retained-memory ownership. Document both guarantees separately. |
| RTIF6 | Wasmtime retained stores have a separate optional-tenant authority key and no retirement interface. | Source-confirmed; current server callers provide a tenant. | Consume the same backend-neutral owner/revocation module as V8. Ownerless retained-store execution fails closed. |
| RTIF7 | V8 partition keys, realm-lease owner/authority, and Wasmtime authority keys independently encode overlapping reuse facts. | Source-confirmed duplication. | Deepen one retained-state ownership module and make backend adapters supply only backend-specific reusable payload/key facts. |
| RTIF8 | V8 warm retention has a worker-global cap but no structurally enforced per-owner partition/cap in the live warm-pool implementation. PIR documents owner/tenant caps more strongly than the code provides. | Source-confirmed. Adaptive replay has tenant-cap math, and realm-lease policy has unused/default-unbounded owner hooks, but the live V8 pool is a flat global vector. | Add explicit owner partitions, owner accounting, owner eviction, and an internally derived per-owner cap. |
| RTIF9 | Base runtime limits and executors remain adapter-registry-owned. Deploy code recovers runtime limits from whichever previous adapter registry exists. | Source-confirmed in Convex/Cloud Functions registries and `nimbus-compute::deploy`. | Make compute own a canonical runtime manager/config; adapters retain artifact and compatibility semantics only. |
| RTIF10 | Current Nimbus does not use `v8::IsolateGroup`; the pinned `rusty_v8` tag does not expose the required shipped integration. | Source and dependency-pin confirmed. | Do not make `IsolateGroup` part of this correctness fix. Keep it behind the existing deferred validation gate. |
| RTIF11 | Engine owns the durable tenant incarnation and operation/delete gate, while the draft runtime-manager shape independently owned incarnation minting and `Active -> Retiring -> Retired`. Two lifecycle authorities would permit split-brain admission and partial-delete states. | Review-confirmed against PR #224 and the current HTTP tenant handler. | Keep Engine/storage authoritative for tenant existence, incarnation, and operation fencing. Make `nimbus-compute` the orchestration boundary that couples an Engine-issued tenant runtime lease to runtime-manager revocation, service teardown, and final Engine deletion. |

### What the existing tests prove

The focused tests run during this review proved:

- default tenant affinity produces distinct V8 reuse keys for two tenant IDs;
- the existing V8 warm-pool cross-tenant test passes for two different tenant
  labels under the default policy.

They do not prove:

- `None` routing affinity isolation;
- unscoped `Script` routing affinity isolation;
- same tenant ID with a new incarnation;
- different tenant IDs whose per-tenant incarnation numbers are equal;
- runtime retirement during a queued or active invocation;
- Wasmtime retained-store owner incarnation isolation;
- full-invocation coupling to Engine's tenant operation/delete gate;
- adapter-independent enforcement through a shared Nimbus runtime manager.

## Non-Negotiable Invariants

1. **No ownerless mutable retention.** A pool kind capable of retaining a
   guest-mutated runtime, isolate, realm substrate, VM, Store, or host resource
   must reject checkout before guest entry unless it has an active owner.
2. **Routing cannot grant authority.** Routing and reuse locality can only add
   partition dimensions. They cannot supply, remove, or replace the owner.
3. **Ownership is incarnation-scoped.** Reusing the same tenant label after
   deletion does not recreate the same runtime owner.
4. **Owner identity is canonical and collision-free.** Tenant runtime-owner
   IDs combine the canonical tenant subject with Engine/storage's durable
   per-tenant incarnation. Compute never allocates a second tenant incarnation,
   and a display label is never an authority key.
5. **One tenant lifecycle authority.** Engine/storage own tenant existence,
   incarnation, and operation/delete fencing. The runtime manager consumes an
   Engine-issued lease and owns only compute admission, cancellation, retained
   state, and worker acknowledgements for that canonical owner.
6. **Ownership is monotonic.** State transitions are
   `bootstrap-clean -> owner-bound -> condemned/destroyed`. Clearing labels,
   resetting request state, creating a realm, or garbage collection cannot
   move owner-bound state back to bootstrap-clean.
7. **Reuse is exact and closed.** Owner, deployment, bundle provenance,
   runtime lane/shape, permission profile, effective capabilities, service
   grants, construction mode, and any backend-specific observable state must
   match. Missing or unclassifiable facts deny reuse.
8. **Revocation wins races.** An invocation checked out before retirement but
   returned afterward is destroyed, not retained. A queued invocation for a
   retired owner never enters guest code.
9. **Owner and authority retirement stay distinct.** Tenant deletion revokes
   an owner incarnation. Deployment replacement retires one reuse-authority
   generation without changing the tenant owner. Both paths condemn matching
   retained state and prevent return-after-retirement reuse.
10. **Purge is not the correctness fence.** Incarnation and revocation prevent
   reuse even if asynchronous eviction is delayed. Purge provides prompt
   reclamation and evidence.
11. **Adapters cannot weaken isolation.** No adapter config, bundle loader,
   runtime-selection rule, or compatibility mode can construct a weaker owner
   or bypass the runtime manager.
12. **Immutable sharing stays explicit.** Bootstrap snapshots, anchors, and
   compiled artifacts remain shareable only because they contain no mutable
   tenant execution state and are exactly keyed/verified.
13. **Hard-security claims stay honest.** Same-process isolates protect normal
    language-level heap separation; they are not protection against a V8,
    native extension, speculative-execution, or process-memory compromise.

## Target Modules And Interfaces

Names below are intent-bearing working names. Implementation may refine them
without weakening the contracts.

### 1. `nimbus-runtime::retained_state`

Create one deep module that owns the retained-state lifecycle. Its external
interface should expose the minimum facts callers and backend adapters need:

- `RuntimeOwnerId`
  - owner class (`tenant`, `system`, or explicit trusted operator/tooling);
  - opaque stable principal/subject ID used for equality and hashing;
  - opaque incarnation scoped to that subject;
  - optional stable display/audit label that never participates in equality.
- `RuntimeOwnerLease`
  - a manager-issued `RuntimeOwnerId`;
  - revocation state shared with queued and checked-out entries;
  - no public constructor that can self-assert tenant/system trust from a
    label.
- `RuntimeReuseAuthority`
  - exact `RuntimeOwnerId`;
  - deployment generation/identity;
  - exact bundle identity and provenance;
  - runtime lane and construction shape;
  - closed effective-capability projection;
  - optional reuse-locality discriminator that can only narrow reuse.
- `OwnerPartitionedPool<T, K>` or an equivalently deep internal module
  - exact-owner checkout and return;
  - rejection of missing/revoked owners;
  - owner-local and global LRU/caps;
  - retirement and pressure eviction;
  - return-after-revocation condemnation;
  - low-cardinality lifecycle metrics.

Two live backend adapters—V8 and Wasmtime—justify this seam. Bun/JSC becomes a
third consumer before retained VM execution is enabled.

Do not expose a caller-constructible “trusted because tenant label matches”
shortcut. `nimbus-runtime` has zero workspace dependencies, so it treats owner
IDs/incarnations as opaque validated values and requires a lease for mutable
retention. `nimbus-compute` lowers Engine/storage's canonical tenant subject and
incarnation into those values. Explicitly owned embedder retention must use a
separate trusted embedding constructor/issuer and an explicit execution-session
generation; fresh execution remains the default ownerless embedder path.

### 2. Separate routing and reuse keys

Replace the overloaded affinity interface with three explicit concepts:

- `RuntimeRouteKey`: optional, best-effort worker placement;
- `RuntimeReuseLocalityKey`: optional extra partitioning by tenant/function/
  script for performance or module-state behavior;
- `RuntimeReuseAuthority`: mandatory for mutable retained state.

The worker router may spill work away from the preferred route. The selected
worker must still use the same mandatory owner and exact reuse authority.

`RuntimeRoutingAffinity::{None,Tenant,Function,Script}` may remain internal
policy vocabulary, but it must lower only to route/locality facts. The owner is
never derived from this enum or from `RuntimeBundleIdentity::tenant_label()`.

### 3. `nimbus-compute::RuntimeManager`

Deepen the existing compute-owned `RuntimeGovernorConfig` into a canonical
Nimbus runtime manager/configuration module. It owns:

- base Nimbus runtime limits and defaults;
- host governor and scaling-plan configuration;
- runtime-lane construction and executor lifetime;
- lane lookup by backend/profile/guest semantics;
- tenant runtime-owner registry populated from Engine-issued leases;
- explicit process-local system/tooling session identity issuance;
- deployment generation registration/retirement;
- runtime owner and deployment-authority leases layered over the Engine lease;
- owner cancellation, drain, retirement, and purge orchestration;
- diagnostics and aggregate metrics.

Adapters provide `RuntimeExecutionRequirements`-shaped data:

- compatibility target;
- guest semantics (`Host`, `ConvexDefault`, or future dialect);
- bundle and entrypoint;
- invocation kind/function metadata;
- adapter-required capabilities already admitted by Nimbus policy.

They receive/use a Nimbus lane handle; they do not construct
`RuntimeExecutor`, clone canonical defaults, or retain runtime configuration.

The manager may share a lane across adapters only when the complete lane key
matches. Different guest semantics, trust profiles, backends, or construction
shapes remain separate lanes. Sharing the executor infrastructure never permits
cross-owner retained-state reuse.

For tenant-backed work the manager does not mint identity. It acquires an
Engine-issued tenant runtime lease that carries the canonical tenant subject,
durable incarnation, and an operation guard held through guest execution and
background drain. The manager attaches its own revocation handle and
deployment-authority lease to that canonical identity.

### 4. Runtime-owner lifecycle

Engine/storage remain authoritative for tenant existence and durable
incarnation. The runtime manager mirrors only the compute-retention state for a
canonical Engine-issued owner:

```text
Absent -> Active(incarnation) -> Retiring -> Retired
             |                     |
             +-- invocation lease -+-- deny new leases
```

Tenant creation allocates the durable incarnation in Engine/storage. Existing
tenant load reuses that value; runtime-manager first use registers it without
advancing or replacing it. A compute-owned tenant lifecycle operation—not the
HTTP handler—must perform deletion in this order:

1. resolve the current canonical Engine tenant subject/incarnation and
   atomically transition its compute state to `Retiring`;
2. reject new runtime-owner leases while preserving Engine's own delete gate as
   the authority for all tenant operations;
3. cancel queued runtime jobs and request cancellation of active jobs;
4. send owner retirement to every runtime lane and every worker;
5. purge worker route entries and idle retained entries;
6. ensure checked-out entries observe revocation and condemn on return;
7. wait for bounded worker acknowledgement and runtime drain; failure leaves
   the owner retiring and the tenant non-recreatable;
8. tear down named runtime-backed resources;
9. invoke/refactor Engine deletion so its operation gate rejects new work,
   drains the full-invocation Engine guards, and removes tenant persistence;
10. mark the compute state `Retired` and remove only reclaimable registry
    state.

Recreating the same tenant label advances the durable Engine/storage
incarnation. The resulting `(subject, incarnation)` cannot compare equal to any
entry, route, queued job, or active lease from the prior tenant. Two different
tenant subjects remain distinct even when both incarnation counters have the
same numeric value.

Deployment replacement uses a separate authority-retirement path. Invocations
admitted before the activation linearization point may finish under the old
generation, but queued/checked-out/returning work remains tagged with that
generation and no old-generation substrate may return to an idle pool after
retirement. New invocations select only the new generation; the tenant owner
itself remains active.

### 5. Backend state classification

| Backend/state | Classification | Required behavior |
| --- | --- | --- |
| V8 startup snapshot | Shareable bootstrap | Exact runtime-shape key; no tenant/user/request payload. |
| NodeFull anchor/read-only heap | Shareable bootstrap | Process/profile infrastructure only; never evaluates tenant bundle. |
| V8 compiled code cache | Immutable derived artifact | Exact engine/profile/bundle/provenance key; no live handles or globals. |
| V8 `WarmPool` | Owner-bound mutable | Exact owner incarnation and reuse authority; module globals intentionally persist only inside that owner partition. |
| V8 `WarmContextRecycle` | Owner-bound substrate | Fresh realm does not remove outer-isolate ownership; exact owner and authority remain required on every lease path, including cooperative execution. |
| Wasmtime compiled component cache | Immutable derived artifact | Shareable under exact content/engine/world key. |
| Wasmtime retained `Store` | Owner-bound substrate | Common owner/revocation contract; exact backend facts; reset host state; discard on error/revocation. |
| Bun/JSC untrusted fresh/discard | Invocation-local | Destroy after invocation; no mutable retention. |
| Bun/JSC trusted retained (future) | Owner-bound mutable | Cannot become product-selectable until it consumes this plan's owner module and proves teardown/timeout safety. |
| Future pre-created clean runtime | Unassigned clean | Globally assignable before guest entry; first guest entry permanently binds owner. |

## Implementation Bands

Execute in order. Each band lands green; fail-before behavior may be captured in
a proof artifact or temporary local commit, but main must not carry deliberately
red tests.

### RTI0 — Reproduction, contract pins, and immediate fail-closed guard

1. Add deterministic behavior tests that demonstrate the current unsafe key
   equivalence/reuse conditions:
   - V8 `WarmPool` with `None` affinity and two tenant owners;
   - V8 `WarmPool` with `Script` affinity, an unscoped bundle, and two owners;
   - same tenant label with two incarnations;
   - Wasmtime retained Store with missing/different owners;
   - tenant delete/recreate with a module-global sentinel through a served
     adapter path.
2. Record fail-before output without weakening or deleting existing tests.
3. Until the owner module and Engine-issued owner lease land, reject mutable
   retained execution through served tenant paths. Checking only for a tenant
   label is insufficient: it would leave `None`, unscoped `Script`, and
   same-label/new-incarnation reuse reachable. Do not silently downgrade an
   explicit retained policy; return a typed contract error. Ownerless
   `StartupSnapshotCache`, immutable compiled/module cache, and direct fresh
   execution remain valid.
4. Add compile-/policy-level pins preventing Bun/JSC retained VM execution from
   becoming product-selectable without an owner contract.

Acceptance:

- all unsafe cases have named tests and a proof note;
- the temporary guard closes RTIF1–RTIF4 rather than only missing-label cases;
- no served retained mutable backend accepts a label in place of an
  Engine-issued owner lease;
- existing fresh/direct embedder paths remain green;
- the guard is documented as temporary scaffolding removed by RTI1/RTI2, not a
  compatibility mode.

### RTI1 — Canonical runtime owner and reuse authority

1. Add `RuntimeOwnerId`, `RuntimeOwnerLease`, and revocation handle types to
   `nimbus-runtime` without adding workspace dependencies. Equality includes
   owner class, opaque stable subject, and incarnation; audit labels do not.
2. Add the owner lease to `RuntimeInvocationContext` or a narrower invocation
   envelope. Keep `tenant_label` only for host binding, fairness, audit, and
   metrics; it is no longer reuse proof. Rename existing `runtime_owner`
   vocabulary for `NimbusRuntime` so the principal term is unambiguous.
3. Introduce the route/locality/authority split. Rename or replace
   `RuntimeAffinityKey` so the implementation cannot accidentally reuse it as
   the owner.
4. Consolidate exact authority facts now split across
   `RuntimePoolPartitionKey`, `RuntimePoolAuthorityKey`,
   `RuntimeRealmLeaseOwner`, and `WasmtimeStoreAuthorityKey`.
5. Use a closed effective-authority projection rather than accidental equality
   on a whole config object. Adding a new authority-affecting runtime field must
   cause a non-exhaustive compile failure or an explicit verifier failure until
   it is classified.
6. Add property tests that mutate every owner/authority dimension one at a time
   and prove keys no longer compare equal. Include two tenant subjects with the
   same numeric incarnation and one tenant subject across two incarnations.
7. Add an Engine-issued tenant runtime lease/projection that exposes the
   canonical subject/incarnation plus an operation guard without moving Engine
   types into `nimbus-runtime`. Compute lowers that projection into the opaque
   runtime owner lease and holds the Engine guard through background drain.

Acceptance:

- mutable retained checkout cannot compile or execute without an explicit
  owner path;
- `None` and unscoped `Script` route/locality choices still produce distinct
  reuse authority for different owners;
- same label/different incarnation never matches;
- different subjects/same incarnation counter never match;
- compute cannot mint or advance a tenant incarnation independently of Engine;
- `nimbus-runtime` retains zero workspace dependencies.

### RTI2 — Owner-partitioned retained-state pools across backends

1. Replace V8's flat warm-entry vector with explicit owner partitions behind
   the common retained-state module.
2. Preserve worker-thread affinity and global pressure/LRU behavior while
   adding per-owner LRU/accounting/caps.
3. Route both `WarmPool` and every `WarmContextRecycle` path through the same
   owner checkout/return contract. Remove the cooperative-path discrepancy in
   which realm lease enforcement is not uniformly exercised.
4. Convert Wasmtime retained Store pooling to the same owner partitions and
   revocation rules. Keep compiled components process-shared.
5. Make Bun/JSC retained-mode interfaces consume the common owner contract even
   while the linked retained implementation remains disabled.
6. Ensure return cleanup checks revocation after guest execution and before
   insertion. Any uncertainty destroys the substrate.

Acceptance:

- two owners may coexist in one worker pool but can only see their own entries;
- owner-local and global eviction are deterministic and metric-correct;
- a fresh realm never changes outer-isolate ownership;
- Wasmtime and V8 share the ownership interface rather than parallel optional
  tenant keys;
- existing warm performance is measured and any regression is recorded.

### RTI3 — Executor retirement and race closure

1. Add acknowledged `RuntimeExecutor::retire_owner` and
   `retire_authority`-shaped interfaces that reach every worker/backend instance
   without violating thread-affine drop. Owner retirement is for tenant
   lifetime deletion; authority retirement is for deployment/config generation
   replacement.
2. Owner retirement purges router affinity entries and removes queued owner
   jobs. Authority retirement purges matching idle/route state; work admitted
   before deployment activation may finish under the old authority but is
   condemned on return.
3. Track checked-out owner and authority work so return-after-retirement is
   condemned.
4. Define bounded owner-retirement active-invocation cancellation/drain
   behavior using existing `HostCallCancellation` and watchdog termination.
5. Ensure backend switches, executor shutdown, and pressure eviction use the
   same condemnation/drop path and do not double-decrement metrics.
6. Add deterministic barrier-based race tests:
   - retire while queued;
   - retire after checkout but before guest entry;
   - retire during guest execution;
   - retire after response-ready but during background drain;
   - return after retirement;
   - concurrent pressure eviction and retirement.
7. Add the equivalent authority-generation races for deployment replacement:
   queued-before-activation, checked-out-old-generation, return after authority
   retirement, and simultaneous owner deletion.

Acceptance:

- retirement acknowledges every worker or returns a fail-closed error;
- no retired job enters guest code;
- no checked-out runtime returns to an idle pool after revocation;
- old deployment authority cannot return to a pool while the tenant's new
  generation remains active;
- thread-affine runtimes are destroyed on their owning worker;
- owner retirement converges router, admission, retained-entry, and active-owner
  counts to zero; authority retirement converges old-authority counts without
  deactivating the tenant owner.

### RTI4 — Compute-owned Nimbus runtime manager and configuration

1. Deepen `RuntimeGovernorConfig` into the canonical runtime manager config,
   including base limits/defaults currently stored in adapter registries.
2. Add the compute-owned runtime manager and runtime lane registry.
3. Key lanes by the complete backend/profile/guest-semantics/construction
   requirements. Reuse matching lane executors across adapters where safe.
4. Move executor construction/lifetime out of `ConvexRegistry` and
   `CloudFunctionsRegistry`.
5. Remove deploy-time logic that discovers canonical limits by inspecting a
   previous Convex or Cloud Functions registry.
6. Preserve overlapping deployment generations safely: new invocations select
   the new generation; old in-flight work drains under its old owner/authority;
   old retained entries are retired explicitly rather than relying on an
   adapter registry `Drop` side effect.
7. Register tenant owners only from Engine-issued runtime leases. The manager
   may mint process-local system/tooling session identities where explicitly
   allowed, but never tenant incarnations.

Acceptance:

- there is one Nimbus source of truth for base limits, defaults, governor, and
  lane executors;
- adding a new adapter does not require copying executor/configuration code;
- deployment with only one adapter, both adapters, or neither produces the same
  canonical runtime defaults;
- redeploy with byte-identical artifacts cannot reuse mutable state from the
  prior generation;
- the runtime-manager registry cannot diverge from Engine/storage's current
  tenant incarnation.

### RTI5 — Tenant lifecycle and adapter migration

1. Add a runtime-manager owner registry keyed by canonical Engine-issued owner
   ID, with atomic `Active -> Retiring -> Retired` compute transitions. It does
   not allocate tenant identity.
2. Make every Nimbus compute invocation acquire both the Engine-issued tenant
   runtime lease and runtime-manager owner/authority lease before runtime
   admission. Top-level and nested invocations preserve the same owner and
   deployment authority, and hold the Engine guard through background drain.
3. Move create/delete tenant orchestration into `nimbus-compute` and keep the
   HTTP handlers thin. Integrate owner retirement before service teardown and
   Engine state removal. A partial failure remains fail-closed, retryable, and
   auditable; same-ID recreation stays rejected while an old owner is retiring.
4. Migrate Convex and Cloud Functions invocation paths to manager-selected
   lanes and explicit owner leases.
5. Cover system-tenant runtime execution with an explicit system owner class;
   do not smuggle `_nimbus` through an ordinary unowned path.
6. Remove adapter registry methods and structs whose only purpose was owning
   runtime policy/executors, including the adapter-centric
   `ConvexRuntimeLane` implementation/name.
7. Add a shared conformance harness that every runtime-using adapter must pass.

Acceptance:

- delete/recreate of the same tenant ID never observes the previous module
  sentinel or retained Store;
- deletion and direct Engine operation admission linearize through Engine's
  existing tenant lifecycle rather than a second compute-only truth;
- nested calls cannot change owner;
- Convex and Cloud Functions pass the same owner/revocation conformance cases;
- no production adapter directly constructs a retained runtime executor;
- tenant deletion has deterministic error behavior when retirement cannot be
  acknowledged.

### RTI6 — Owner fairness, diagnostics, and operational evidence

1. Enforce an internally derived maximum retained entries per owner and per
   worker, plus the existing global/pressure bounds. Do not expose raw pool
   knobs as developer configuration.
2. Add low-cardinality counters for owner checkout, exact hit/miss, revocation,
   owner mismatch denial, return-after-revoke discard, retirement/purge, and
   retirement acknowledgement failure.
3. Add read-only diagnostics showing counts by lane/profile and anonymized or
   redacted owner class. Tenant IDs/incarnations must not become unbounded metric
   labels.
4. Verify that one tenant cannot evict all other tenants' warm entries without
   first exhausting its owner allocation; global pressure may still evict
   fairly across owners.
5. Add a benchmark for owner-partition lookup and retirement fanout at realistic
   tenant/function skew.

Acceptance:

- owner caps are enforced in the live pool, not replay math only;
- metrics balance under hit, miss, error, pressure, retirement, and shutdown;
- diagnostics can explain why an entry was not reused without exposing tenant
  secrets or high-cardinality labels;
- the owner-partition design stays within the accepted latency/RSS envelope.

### RTI7 — Cleanup, documentation truth, and final proof

1. Delete temporary RTI0 guards made redundant by the mandatory owner
   interface. Do not retain compatibility shims.
2. Delete duplicate/ad hoc authority key code and stale dead-code allowances.
3. Update:
   - runtime adapter and trust architecture docs;
   - local enforcement lifecycle docs;
   - isolate glossary;
   - profile-aware runtime findings/final architecture record;
   - tenant-isolation operations documentation;
   - the July multi-tenant audit whose “safe exact key includes tenant” verdict
     is incomplete for `None`, unscoped `Script`, Wasmtime, and tenant
     recreation.
4. Replace “Convex runtime” wording with the terminology decision above.
5. Add `scripts/verify-runtime-tenant-isolation.sh` or extend the existing
   profile-aware verifier with static guards for:
   - no adapter-owned `RuntimeExecutor` fields;
   - no retained key containing only `Option<tenant>` as ownership;
   - no tenant runtime-owner ID lacking both stable subject and canonical
     incarnation;
   - no compute-local tenant-incarnation allocator or label-only tenant owner
     constructor;
   - no owner derivation from routing affinity or bundle tenant label;
   - no product-selectable retained backend without the owner interface;
   - no tenant/user payload admitted into startup snapshots/anchors.
6. Record final proof under
   `docs/private/plans/proof/runtime-tenant-isolation/`.

Acceptance:

- docs describe the shipped behavior, not the intended behavior;
- static guards prevent the architecture from drifting back;
- focused runtime/compute/server/adapter suites, runtime harness, formatting,
  clippy, and `make ci` are green with named counts/evidence;
- every finding RTIF1–RTIF11 is closed or explicitly retained as a correctly
  gated non-goal.

## Required Test Matrix

| Dimension | Required cases |
| --- | --- |
| Owner | different tenant IDs; different tenants with the same numeric incarnation; same tenant/same incarnation; same tenant/new incarnation; system vs tenant; missing owner; forged label-only owner; revoked owner |
| Routing | `None`, `Tenant`, `Function`, `Script`; worker affinity hit; least-loaded spill; affinity-cache eviction |
| Bundle | scoped/unscoped; same bytes/same deploy; same bytes/new deployment; changed digest; changed entrypoint; missing expected hash where filesystem trust is explicitly allowed |
| Runtime state | module-global sentinel; closure/singleton cache; host-call state; pending `waitUntil`; dirty resource table; clean realm; retained Wasmtime Store |
| Authority | permission profile; exact service grants; filesystem projection; egress posture; compatibility target; guest semantics; construction mode; backend kind |
| Lifecycle | Engine-operation lease held through background drain; queued cancellation; active cancellation; response-ready/background drain; delete/recreate; partial-delete retry; redeploy overlap/authority retirement; executor shutdown; pressure eviction; max reuse |
| Adapter | Convex query/mutation/action/HTTP action; Cloud Functions HTTP/trigger; system-tenant path; direct embedder fresh and explicitly owned retained paths |
| Backend | V8 `WarmPool`; V8 `WarmContextRecycle`; V8 snapshot cache; Wasmtime retained Store; Wasmtime compiled cache; Bun/JSC fresh discard; retained Bun/JSC admission guard |

Behavior tests must assert data/state outcomes, not only hit/miss counters. The
cross-owner tests write a secret sentinel into mutable guest state under owner A
and prove owner B/new incarnation cannot observe it. Same-owner tests prove the
intended warm behavior still works.

## Scope And Likely Files

Primary implementation locality:

- `crates/nimbus-runtime/src/context.rs`
- `crates/nimbus-runtime/src/affinity.rs`
- `crates/nimbus-runtime/src/execution_plan.rs`
- `crates/nimbus-runtime/src/backends/v8/warm_pool.rs`
- `crates/nimbus-runtime/src/backends/wasmtime/store_pool.rs`
- `crates/nimbus-runtime/src/backends/bun_jsc/`
- `crates/nimbus-runtime/src/runtime/realm_lease.rs`
- `crates/nimbus-runtime/src/executor/`
- `crates/nimbus-runtime/src/worker_loop/`
- new concept-owned retained-state modules under `nimbus-runtime`
- `crates/nimbus-compute/src/config/runtime.rs`
- `crates/nimbus-compute/src/state.rs`
- `crates/nimbus-compute/src/deploy.rs`
- new compute-owned tenant lifecycle orchestration
- `crates/nimbus-compute/src/execution/invocations/`
- `crates/nimbus-engine/src/engine/tenants.rs`
- `crates/nimbus-engine/src/tenant.rs`
- `crates/nimbus-convex/src/registry/`
- `crates/nimbus-cloud-functions/src/registry.rs`
- `crates/nimbus-server/src/http/tenants.rs`
- Convex and Cloud Functions server invocation adapters/tests

Respect these architecture invariants:

- `nimbus-runtime` keeps zero workspace dependencies;
- adapters remain thin compatibility modules over Nimbus runtime primitives;
- HostBridge operations remain decision-derived and rechecked per operation;
- Engine/storage remain the only tenant-incarnation allocator and tenant
  operation/delete authority;
- no new public efficiency knobs expose pool kind, routing affinity, reset
  strategy, isolate group, or executor topology;
- no compatibility layer is required because Nimbus is pre-launch.

## Coordination

- The completed profile-aware isolate runtime plan owns prior measurement and
  density decisions; this plan owns correctness reconciliation and retirement.
- `layered-admission-control-plan.md` continues to own node-wide host pressure
  and aggregate admission. This plan owns only retained-state owner partitions,
  owner caps, and lifecycle accounting needed for correctness/fairness.
- `horizontal-scaling-plan.md` may later bind the canonical tenant
  subject/incarnation to workload UID/generation and distributed placement
  leases. This plan must land first and must not wait for cluster mode.
- Adapter compatibility plans own protocol semantics only. They consume this
  plan's runtime manager and conformance harness.
- The active architecture-review plan records its own earlier campaign. This
  later dedicated plan is the sole owner for the findings listed here.
- PR #224's durable tenant incarnation and Engine tenant lifecycle are upstream
  authorities consumed by this plan, not seams to replace. If their public
  projection needs deepening, this plan owns only the runtime-lease integration
  needed for RTIF4/RTIF11.

## Rejected Designs

### One executor/thread pool per tenant

Rejected as the default. It multiplies threads, queues, snapshots, anchors, and
idle memory by tenant count. Explicit owner partitions inside shared
thread-affine workers provide the required language-level memory separation
with substantially better density. A process/microVM-per-tenant tier remains
valid when the threat model requires a kernel boundary.

### `IsolateGroup` per tenant

Rejected as the correctness mechanism. It does not prevent selection of the
wrong retained `JsRuntime`, and it remains a same-process V8 grouping/cage
primitive. Per-tenant groups would duplicate/reserve group resources and reduce
density without replacing owner keys, revocation, or lifecycle retirement.

### Clear globals or create a fresh realm, then mark unowned

Rejected. Nimbus cannot prove that arbitrary V8/native/embedder state has
returned to pristine bootstrap state. Reset is useful inside one exact owner;
it is not a tenant declassification operation.

### Purge by tenant label only

Rejected. Purge races with queued and checked-out work, and the same label can
be recreated. Incarnation/revocation is the correctness fence; purge is
reclamation.

### Mint a runtime-specific tenant incarnation

Rejected. Engine/storage already own a durable per-tenant incarnation that
survives tenant-database deletion and is shared by embedded and external
providers. A second compute-local counter creates split-brain identity. Runtime
owner equality uses the canonical tenant subject plus that incarnation.

### Use decision ID, tenant label, or deployment generation as the incarnation

Rejected. These can repeat when the same tenant is deleted/recreated under an
unchanged deployment and policy. Deployment remains a separate reuse-authority
dimension; labels remain non-authoritative metadata.

### Keep executors in adapter registries

Rejected. It makes canonical configuration discoverable only by inspecting an
arbitrary adapter, duplicates lane construction, and lets future adapters
accidentally define weaker isolation. The compute-owned manager provides a
smaller interface with greater leverage and locality.

## Nice-To-Have Follow-Ons (Not Completion Gates)

These are explicitly outside RTI0–RTI7 completion unless implementation work
uncovers a correctness dependency:

1. Replace the current raw positive `u64` tenant-incarnation projection with a
   dedicated cross-crate newtype if broader Engine/storage work makes that
   worthwhile. RTI correctness consumes the existing durable value and does not
   wait for this type polish.
2. Add a globally shareable pool of pre-created bootstrap-clean runtimes if a
   benchmark proves benefit. Assignment remains one-way to an owner.
3. Add operator diagnostics that render a redacted owner-partition occupancy
   map and retirement history.
4. Evaluate isolate groups only after the pinned `rusty_v8` and `deno_core`
   interfaces, snapshot compatibility, cage sizing, and measurements are
   proven. Group by compatible VM shape or an optional hardened tier, not
   automatically by tenant.
5. Offer process- or microVM-isolated runtime tiers for tenants requiring
   protection beyond same-process isolate semantics.
6. Add adversarial/fuzz scheduling that repeatedly interleaves owner retirement,
   worker spill, pressure eviction, and deployment replacement.
7. Benchmark whether owner-local segmented LRU, CLOCK-Pro, or another policy
   improves skewed-tenant hit rate after the simple correct owner partitions
   land. Correctness keys are never benchmark knobs.

## Status Ledger

| Band | Status | Evidence required before `done` |
| --- | --- | --- |
| RTI0 — reproduction and guard | `todo` | fail-before proof, named behavior tests, served mutable-retention rejection |
| RTI1 — owner/authority interface | `todo` | key property matrix, zero-workspace-dependency proof |
| RTI2 — owner-partitioned pools | `todo` | V8/Wasmtime behavior tests, Bun retained guard, benchmark delta |
| RTI3 — executor retirement | `todo` | deterministic race matrix and worker acknowledgement proof |
| RTI4 — runtime manager/config | `todo` | adapter-independent lane/config tests, redeploy overlap proof |
| RTI5 — lifecycle/adapters | `todo` | served delete/recreate test and adapter conformance matrix |
| RTI6 — fairness/diagnostics | `todo` | cap/fairness/metric-balance tests and skew benchmark |
| RTI7 — cleanup/final proof | `todo` | docs/static verifier/focused suites/`make ci` evidence |

## Completion Gate

This plan is complete only when:

- all RTI0–RTI7 rows are `done` with named evidence;
- no mutable retained backend can execute without a runtime owner;
- routing choices cannot remove or synthesize owner authority;
- tenant runtime-owner IDs come only from the canonical Engine/storage subject
  and incarnation, with no label-only or compute-local allocator;
- delete/recreate of the same tenant ID is proven free of old guest state;
- retirement races cannot return revoked state to any idle pool;
- V8 and Wasmtime use the shared owner-partition interface, and Bun retained
  execution is gated on it;
- compute owns canonical runtime configuration, lane executors, runtime
  revocation, and tenant lifecycle orchestration while Engine/storage remain
  authoritative for tenant incarnation and operation/delete fencing;
- Convex and Cloud Functions are adapters over that Nimbus-owned runtime;
- shareable bootstrap/compiled artifacts remain tenant-state-free;
- the IsolateGroup decision remains explicitly separate and honest;
- documentation, the July audit, and static architecture guards match shipped
  behavior;
- `cargo fmt --all --check`, focused crate suites, the runtime verification
  harness, `make clippy`, and `make ci` pass with recorded counts/output.

## Paste-Ready Implementation Goal

```text
/goal Execute docs/private/plans/nimbus-runtime-tenant-isolation-plan.md in order RTI0 through RTI7. Preserve nimbus-runtime's zero-workspace-dependency invariant, Engine/storage's sole authority over tenant incarnation and operation/delete fencing, and thin adapters. First pin the None-affinity, unscoped-Script, same-label/new-incarnation, Wasmtime retained-Store, and served delete/recreate failures; until the canonical owner lease lands, reject served mutable retention rather than treating a tenant label as proof. Introduce opaque RuntimeOwnerId/RuntimeOwnerLease types whose equality includes owner class, stable subject, and incarnation; lower tenant owners only from an Engine-issued runtime lease, prove different subjects with the same per-tenant incarnation counter remain distinct, and split routing locality from reuse authority. Refactor V8 and Wasmtime retained state into owner partitions, gate Bun/JSC retained execution, add acknowledged owner and deployment-authority retirement across every worker, and prove queued/in-flight/return-after-revoke races fail closed. Deepen nimbus-compute's RuntimeGovernorConfig into the canonical Nimbus RuntimeManager so base limits, defaults, lanes, executors, runtime revocation, deployment retirement, and diagnostics are Nimbus-owned rather than stored in Convex or Cloud Functions registries; do not mint a parallel tenant incarnation. Move tenant lifecycle orchestration into compute, hold the Engine operation guard through guest/background drain, and migrate Convex and Cloud Functions through one conformance harness. Add owner caps and low-cardinality evidence, delete temporary/duplicate authority code, truth up the stale isolation docs/audit, and add a static verifier. Do not use IsolateGroup as the tenant boundary, do not add public pool knobs, and do not retain compatibility shims. Update each ledger row with exact tests/counts and proof artifacts. Finish only when RTI0-RTI7 are done and cargo fmt --all --check, the focused runtime/compute/server/adapter suites, make verify-harness SURFACE=runtime, make clippy, and make ci are green.
```
