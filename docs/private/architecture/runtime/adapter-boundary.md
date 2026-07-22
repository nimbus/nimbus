# Runtime Capability And Adapter Boundary

This reference defines the intended ownership boundary between adapter-owned
runtime compatibility shims and provider-neutral runtime capabilities inside
`crates/nimbus-server/`.

It complements:

- [ARCHITECTURE.md](../../../ARCHITECTURE.md)
- [Cloud Functions compatibility](../../adapters/cloud-functions/compatibility.md)
- [Cloudflare adapters operator notes](../../operating/cloudflare-adapters.md)
- [Firebase compatibility](../../adapters/firebase/compatibility.md)
- [runtime-capability-adapter-boundary-plan.md](../../plans/archive/runtime-capability-adapter-boundary-plan.md)
- [server-runtime-canonicalization-plan.md](../../plans/archive/server-runtime-canonicalization-plan.md)
- [Node systemd D-Bus binding](../../operating/node-dbus-binding.md) — the
  live `SystemdDbusClient` host-lifecycle binding behind the node boundary

## Why This Boundary Exists

Nimbus now supports multiple adapter families. That only stays maintainable if
provider-specific compatibility logic remains adapter-owned and the reusable
execution behavior below it stays provider-neutral.

The intended model is:

1. `nimbus-core`, `nimbus-engine`, and `nimbus-storage` own canonical data and
   execution primitives.
2. `runtime_host/*` owns provider-neutral server-side runtime capabilities.
3. `adapters/*` own provider-specific compatibility shims, including runtime
   API shims.
4. adapters translate provider contracts into shared capabilities; shared
   capabilities do not absorb provider contracts.

## Ownership Rules

### Compute Owns Runtime Lanes And Reuse Authority

`nimbus-compute::RuntimeManager` is the composition root for Nimbus runtime
configuration, lane executors, runtime-owner admission, deployment-authority
generation, retirement, and diagnostics. A runtime lane is keyed by the full
backend/profile/guest-semantics/construction requirements. Convex and Cloud
Functions select those requirements and provide adapter-specific bundles and
host semantics; their registries do not own `RuntimeExecutor`, base runtime
limits, pool policy, or reuse authority.

Tenant owners come only from an Engine-issued tenant runtime lease. The
authority-bearing identity is owner class plus stable tenant subject plus the
Engine/storage incarnation; the human-readable tenant label remains audit and
fairness metadata. Routing affinity may improve worker locality, but it cannot
create, replace, or weaken that owner identity. Deletion and deployment
replacement are acknowledged by every lane worker before their retained state
is considered retired.

### Adapters Own Provider-Specific Runtime Shims

Provider-specific runtime APIs belong under adapter ownership, even when they
are invoked from executed JavaScript instead of over HTTP or WebSocket.

Examples:

- `firebase-admin/firestore`
- Convex `ctx.db.*` contract lowering
- Cloudflare Workers `env` bindings, KV namespace methods, and Durable Object
  stub semantics
- provider-shaped response or error payloads
- provider-specific path parsing, identifier validation, and option semantics

These are still adapter compatibility surfaces. The fact that they run through
the runtime does not make them generic primitives.

### `runtime_host/*` Owns Provider-Neutral Capabilities

Shared runtime-host modules may provide primitive capabilities such as:

- document reads by canonical locator
- staged atomic write-batch execution
- standalone write-batch execution
- invocation/session validation
- principal and tenant access
- read tracking
- generic runtime result structs

These capabilities must not carry Firebase-, Firestore-, or Convex-specific
payloads, names, or response shapes.

### Egress Is A Tier-Neutral Runtime Seam

Outbound HTTP egress is not adapter-owned. Container, isolate, and wasm traffic
share one tenant policy decision and one forwarding/enforcement model:

- `nimbus-runtime` owns only the zero-workspace-dependency `EgressGateway`
  trait and seam types, plus substrate binding adapters for isolate `fetch` and
  wasm/WASI HTTP.
- `nimbus-server` implements `EgressGateway` by adapting runtime requests to
  admitted tenant policy and readiness state.
- `nimbus-egress` is the pure PDP that decides policy. It depends on
  `nimbus-core` only and does not own sockets, DNS, TLS, forwarding, or secret
  material.
- `nimbus-proxy` is the PEP that enforces the decision with DNS and canonical
  authority checks, forwarding, credential injection, DLP, pool identity, and
  redacted decision logs.
- `nimbus-sandbox` wires container workloads to the PEP; it must not regain a
  parallel egress policy or forwarding stack.

Provider adapters may request egress through the same runtime/tenant authority
model, but they must not define provider-specific bypasses around the PDP/PEP
split.

### Object Storage Is Native; S3 Is An Edge Adapter

Object storage follows the same adapter-boundary rule:

- `nimbus-storage` owns object manifests and multipart metadata through
  `ObjectMetaStore`.
- `nimbus-blob` owns immutable bytes, local packs, object-store byte legs,
  placement composition, GC, and backup chunks.
- `nimbus-object-storage` owns placement-policy resolution and provider config.
- `nimbus-s3` owns S3 protocol semantics only.
- `crates/nimbus-server/src/adapters/s3` owns listener, guard, and router
  integration only.

Convex `_storage`, the S3 front door, backup/restore, future R2 compatibility,
and the NimbusFS object mount must consume those native primitives. They must
not each grow their own object model or make the S3 listener the placement owner.

### Translation And Execution Are Separate

Provider shims may translate:

- Firestore document paths into Nimbus document locators
- provider request payloads into generic field maps or write batches
- provider options into generic execution flags

Shared capabilities then execute those generic inputs and return generic
results. Provider shims own the last-mile translation back into
provider-observable result shapes.

### Tenant Bundles Stay Non-Operator Realms

Tenant runtime bundles are a tenant/spawned-workload realm. They may import the
high-level `@nimbus/nimbus` SDK and authenticate through workload identity, but
they must not import low-level or operator-only transport entries such as
`@nimbus/nimbus/transports/rest`, and they must not package local-admin tokens,
operator session material, static Nimbus control-plane tokens, or
`LocalAdminTokenRecord`-class credential objects.

The guard is enforced in two places:

- codegen tenant bundle admission rejects static imports, dynamic imports,
  requires, and re-exports of operator-only Nimbus transport entries, plus
  obvious operator credential markers;
- the V8 runtime module loader repeats the operator-only transport denylist so a
  bundle cannot reach that low-level path by bypassing codegen.

This is a realm-separation rule, not a substitute for server authorization:
route policy still resolves the caller's principal class and exact grants
server-side.

## Naming Rules

- Shared modules must use provider-neutral names.
  Good examples: `documents.rs`, `writes.rs`, `session.rs`, `capabilities.rs`.
- Shared runtime ABI operation names must also stay provider-neutral.
  The generic document lane now uses `DocumentGet`, `DocumentInsert`,
  `DocumentPatch`, and `DocumentDelete`; adapter-specific names like
  `convex.ctx.db.get` stay at adapter-owned wire or contract edges.
- Provider-specific names such as `firestore`, `firebase_admin`, or `convex`
  belong under adapter-owned modules unless there is a very strong documented
  exception.
- Moving a provider-named file into a shared directory does not make it shared.
  If the inputs, outputs, or dependencies are still provider-specific, the file
  is still an adapter shim.

## Dependency Rules

- Modules under `runtime_host/*` may not depend directly on adapter-owned types
  such as `ConvexHostBridge`, `ConvexRegistry`, or adapter-specific response
  envelopes.
- Adapter modules may depend on shared runtime capability traits or functions.
- Shared runtime capability modules may depend on core, engine, storage, and
  provider-neutral server types.

## Boundary Mistakes This Reference Calls Out

These examples capture the kinds of ownership mistakes this boundary is meant to
prevent:

- historical extraction mistake:
  `crates/nimbus-server/src/runtime_host/firestore_admin.rs`
  - provider-specific `firebase-admin/firestore` shim logic was temporarily
    placed under the shared runtime-host tree instead of adapter ownership
- `crates/nimbus-server/src/runtime_host/mod.rs`
  - a nominally shared runtime host implemented as a thin wrapper around
    Convex-owned bridge and registry types
- `crates/nimbus-server/src/adapters/convex/host_bridge/db_ops/mod.rs`
  - Convex host-bridge dispatch carrying `FirebaseAdminFirestore*` host calls

These examples are not the target architecture.

## Target End-State

The steady-state layout should look like:

- shared runtime capabilities under `runtime_host/*`
  - provider-neutral execution primitives only
- provider-specific runtime compatibility shims under `adapters/*`
  - Cloud Functions-owned `firebase-admin/firestore` shim
  - Convex-owned `ctx.db.*` shim
- adapter composition roots that adapt shared capabilities while the
  compute-owned `RuntimeManager` owns runtime lanes and reuse authority

In short:

- adapters are the shim layer
- shared capability code is the primitive layer
- provider-specific names stay with the shim layer

## Current Landed Layout

The current corrected layout is:

- shared runtime primitives under `runtime_host/*`
  - `runtime_host/capabilities.rs`
  - `runtime_host/abi/document_calls.rs`
  - `runtime_host/abi/mod.rs`
  - `runtime_host/responses.rs`
  - `runtime_host/mod.rs`
  - `runtime_host/abi/document_calls.rs` now dispatches generic
    `Document*` host-call payloads instead of Convex-branded `CtxDb*` names
- Cloud Functions-owned runtime compatibility shims under
  `adapters/cloud_functions/*`
  - `adapters/cloud_functions/host_bridge.rs`
  - `adapters/cloud_functions/runtime_api/firebase_admin/firestore.rs`
- Convex-owned runtime compatibility shims under `adapters/convex/*`
  - Convex `ctx.db.*` dispatch stays adapter-owned, translates from the
    generic `Document*` runtime ABI lane, and no longer carries
    `FirebaseAdminFirestore*` host calls
- Cloudflare-owned runtime compatibility shims under `adapters/cloudflare/*`
  - Workers KV REST and `env.NS` binding calls translate to the
    provider-neutral `TenantKvStore` primitive.
  - Durable Object identity, leases, alarms, and hibernation live in the
    Cloudflare adapter/server substrate and are keyed by
    `(tenant_id, do_namespace, do_id)`.
  - `nimbus-runtime` exposes only the minimal host-call and Worker-dispatch ABI;
    it does not depend on Nimbus workspace crates or own Cloudflare-specific
    storage/auth policy.

This means the repo no longer treats provider-specific runtime shims as shared
primitives. Shared runtime-host code now owns only provider-neutral execution
capabilities and server-owned context types.
