# Runtime Capability And Adapter Boundary

This reference defines the intended ownership boundary between adapter-owned
runtime compatibility shims and provider-neutral runtime capabilities inside
`crates/nimbus-server/`.

It complements:

- [ARCHITECTURE.md](../../ARCHITECTURE.md)
- [cloud-functions-compatibility.md](cloud-functions-compatibility.md)
- [firebase-compatibility.md](firebase-compatibility.md)
- [runtime-capability-adapter-boundary-plan.md](../plans/runtime-capability-adapter-boundary-plan.md)
- [server-runtime-canonicalization-plan.md](../plans/server-runtime-canonicalization-plan.md)

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

### Adapters Own Provider-Specific Runtime Shims

Provider-specific runtime APIs belong under adapter ownership, even when they
are invoked from executed JavaScript instead of over HTTP or WebSocket.

Examples:

- `firebase-admin/firestore`
- Convex `ctx.db.*` contract lowering
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

### Translation And Execution Are Separate

Provider shims may translate:

- Firestore document paths into Nimbus document locators
- provider request payloads into generic field maps or write batches
- provider options into generic execution flags

Shared capabilities then execute those generic inputs and return generic
results. Provider shims own the last-mile translation back into
provider-observable result shapes.

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
- adapter composition roots that adapt shared capabilities instead of acting as
  the accidental shared runtime owner

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

This means the repo no longer treats provider-specific runtime shims as shared
primitives. Shared runtime-host code now owns only provider-neutral execution
capabilities and server-owned context types.
