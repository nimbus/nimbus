# Runtime Engine Seam

Status: active baseline

This reference defines the internal Nimbus seam for adding runtime engines
beside the current Deno/V8 implementation. It is intentionally engine-neutral:
the same boundary should be able to host the existing `deno_core` + V8 path, a
future Bun/JSC path, and non-JavaScript backends such as wasmtime without
making any one engine's setup rules look generic.

It complements:

- [ARCHITECTURE.md](../../../ARCHITECTURE.md)
- [Runtime Capability And Adapter Boundary](adapter-boundary.md)
- [Runtime Permission Model](permission-model.md)
- [Node compatibility surface matrix](node-compat-surface-matrix.md)
- [New runtime engine proof harness](new-engine-proof-harness.md)
- [Runtime engine seam plan](../../plans/runtime-engine-seam-plan.md)

## Why This Seam Exists

The current runtime crate already has a worker-facing backend trait, but most
of the actual execution contract below it is Deno/V8-shaped. `NimbusRuntime`
constructs `deno_core::JsRuntime`, installs Deno extensions and Deno ops,
loads V8 startup snapshots, drives a V8 event loop, and terminates execution
through a V8 isolate handle.

That is the correct implementation for the existing JavaScript runtime family,
but it is not the correct abstraction for new engines. A Bun/JSC backend would
need JSC host functions, Bun's module loader and event loop, Bun/JSC
termination hooks, and different VM reuse rules. A wasmtime backend would need
typed imports, Stores, components, fuel or epoch interruption, and a different
memory limiter. Neither should pretend to be a Deno runtime.

The runtime engine seam therefore separates these axes:

| Axis | Meaning | Examples |
| --- | --- | --- |
| Engine | The embedded execution implementation and VM ownership model. | Deno/V8 today; Bun/JSC or wasmtime later. |
| Compatibility target | The JavaScript or guest API contract exposed to user code. | `WebStandardIsolate`, `Node20`, `Node22`, `Node24`, future Bun-compatible target. |
| Execution model | How a worker drives progress and scheduling. | run-to-completion, cooperative V8 Locker, future fuel/epoch or engine-specific cooperative loops. |
| Pooling model | What is retained between invocations. | V8 warm pool, startup snapshot cache, fresh VM, component cache, future engine-local pools. |
| Permission policy | The host resources the invocation may access. | Runtime mode plus `RuntimeGrants`; independent of engine and compatibility target. |

## Layering Rules

The primary executor seam remains `WorkerLoopFactory` / `WorkerLoop`. Worker
loops own admission, queue draining, cancellation wiring, and the execution
model. Engine-specific runtimes live below the worker loop.

```text
RuntimeExecutor
  -> WorkerLoopFactory
      -> WorkerLoop
          -> RuntimeBackendFactory
              -> RuntimeBackend
                  -> RuntimeEngine
```

The names are deliberately layered:

- `WorkerLoop` is the scheduler-facing seam.
- `RuntimeBackend` is a worker-local adapter selected by runtime policy.
- `RuntimeEngine` is the engine-specific VM owner below the backend.
- `HostBridge` is the stable Rust host ABI used by guest code.

`RuntimeBackend` may remain the public worker-local helper, but it must not
require callers to pass a Deno/V8 runtime instance. The backend owns whatever
engine state it needs: a Deno/V8 warm pool, a Bun/JSC VM wrapper, a wasmtime
component cache, or a fresh-per-invocation runtime.

## Backend-Agnostic Invocation Envelope

The invocation envelope passed from the worker loop to a backend should contain
only engine-neutral inputs:

- verified runtime policy and grants
- watchdog and external cancellation handles
- runtime bundle identity and integrity inputs
- invocation request and context
- shared host-call permit
- host bridge handle
- metrics and tracing correlation data

It should not contain:

- `deno_core::JsRuntime`
- V8 handles, isolates, lockers, globals, or snapshots
- Deno `OpState`, extensions, permission containers, or module loaders
- Bun/JSC `VirtualMachine`, `JSGlobalObject`, or event-loop handles
- wasmtime `Store`, `Engine`, `Component`, or `Linker`

Those objects are backend-owned. The worker loop may route, park, wake, and
finish work, but it should not reach through the backend into VM internals
except through a deliberately engine-specific cooperative driver.

`RuntimeWorkerJob` and any successor queue envelope should follow the same
rule. It may carry the stable host bridge, policy, bundle, request, context,
and cancellation state, but it should not become the hidden place where one
engine's runtime object is threaded through the supposedly generic executor.

## Artifact And Bundle Rules

Runtime bundle identity is part of the engine seam. The current bundle type is
JavaScript-ESM-shaped: it stores a file entrypoint, a V8 module specifier, a
bundle-root-derived module policy, and a Deno/V8 module code cache. That is
correct for the current engine, but it is not the final generic bundle
contract.

Future runtime-extension work must separate:

- bundle identity: tenant label, canonical entrypoint, expected SHA-256, and
  engine-relevant cache keys
- bundle content kind: JavaScript module, Bun/JSC JavaScript module, WASM
  component, or another explicit future kind
- engine-owned derived state: V8 module specifiers and code cache, Bun module
  loader state, wasmtime component cache entries, and engine configuration hash
- deploy/codegen metadata: compatibility target, engine selection, external
  package policy, component world, and evidence lane

Bundle integrity remains mandatory for every content kind. Engine caches must
be invalidated by content hash plus engine configuration, not by path alone.

Do not overload the existing Node fields in generated artifacts to select a new
engine. A Bun-backed target needs explicit artifact metadata; a wasmtime
component needs explicit component metadata.

## Host-Call ABI And Runtime Extensions

`HostBridge` is the Rust-side ABI, and `HostCallOperation` / `HostCallPayload`
define the versioned operation family that engines must preserve. The transport
is engine-specific:

- Deno/V8 transports host calls through Deno ops.
- Bun/JSC should transport host calls through JSC host functions.
- wasmtime should project host calls into typed WIT imports.

The transport must preserve:

- ABI version rejection
- operation/payload mismatch rejection before adapter dispatch
- sync versus async call behavior
- cancellation propagation
- `SharedInvocationPermit` pause/resume semantics during async host I/O
- host-operation metrics and tracing correlation

The generic `RuntimeExtensionCall` lane is also part of this ABI. Engine work
must carry that provider-neutral extension lane without moving adapter-owned
namespaces into `nimbus-runtime`. Cloud Functions may own a
`cloud_functions` namespace, Convex may reject unsupported adapter extension
calls, and future adapters may add their own namespaces; the engine transport
must not hard-code those adapter meanings.

## Server Adapter And Codegen Rules

Runtime engine selection crosses the codegen and server registry boundary, not
just the VM boundary. The current Convex-compatible artifact model is
Node-specific: `"use node"` modules emit `runtime_environment = "node"` plus a
`node_runtime_target`, and the server routes those functions to Node20, Node22,
or Node24 runtime lanes.

Before a new engine becomes selectable:

- codegen must emit explicit engine or content-kind metadata rather than
  overloading `runtime_environment = "node"`
- the function manifest must distinguish compatibility target from engine
  implementation
- external package metadata must state which engine owns package resolution and
  loading
- server registries must route functions by validated engine/target policy, not
  by Node-only fields
- operator and metadata APIs must expose the selected engine and compatibility
  target honestly

The default Deno/V8 lane should remain unchanged while these fields are added.

## RuntimeEngine Responsibilities

Each engine implementation owns the following responsibilities behind a common
conceptual contract:

| Responsibility | Deno/V8 implementation today | Future Bun/JSC equivalent | Future wasmtime equivalent |
| --- | --- | --- | --- |
| VM construction | `deno_core::JsRuntime` options, V8 snapshots, Deno extensions. | Bun/JSC VM initialization without Bun CLI process ownership. | `wasmtime::Engine`, component compilation, Store creation. |
| Host calls | Deno ops backed by `HostBridge`. | JSC host functions backed by `HostBridge`. | WIT imports backed by `HostBridge` or typed capability adapters. |
| Bootstrap | Deno-op-backed JS installs `__nimbusCreateContext` and runtime globals. | JSC-host-function-backed JS installs the same Nimbus context contract. | Guest ABI bindings expose typed host capabilities. |
| Module loading | Restricted `deno_core` loader, Node resolver, code cache. | Bun module loader/evaluator with Nimbus bundle root and package policy. | Component/module cache keyed by bundle hash. |
| Event-loop progress | Deno/V8 event loop polling and promise settlement. | Bun/JSC event loop driving and promise settlement. | Store call/fuel loop or async component execution. |
| Timeout and cancellation | V8 isolate termination through the shared watchdog. | JSC execution time limit and termination request. | Fuel/epoch interruption or Store cancellation. |
| Memory limits | V8 heap limits and near-heap-limit handling. | JSC/Bun heap limit strategy or discard-on-pressure policy. | Store resource limiter. |
| Reuse | V8 warm-pool reset and stale-context guards. | Bun VM reuse proof, or fresh/discard-only mode. | Component cache with fresh Store, or retained Store reset. |
| Teardown | V8 deferred drop queue and locker-aware destruction. | Bun VM destroy path that does not call process exit. | Store drop and cache eviction. |

The common contract is behavioral, not type-level inheritance from Deno/V8.
The implementation type for each row may be completely different.

## Bootstrap Split

Nimbus currently installs most JavaScript runtime context through a bootstrap
source file that calls `Deno.core.ops`. That source is partly generic Nimbus
contract and partly Deno/V8 plumbing. Before adding another JavaScript engine,
split bootstrap ownership into two layers:

1. **Engine-neutral Nimbus context contract**
   - installs `__nimbusCreateContext`
   - defines stale-context generation checks
   - builds `ctx.db`, `ctx.scheduler`, `ctx.runQuery`, `ctx.runMutation`,
     `ctx.runAction`, service-binding access, and runtime error normalization
   - depends only on injected sync and async host-call primitives

2. **Engine-owned host-call transport**
   - Deno/V8: injected primitives call Deno ops
   - Bun/JSC: injected primitives call JSC host functions
   - wasmtime: equivalent contract is typed imports rather than JavaScript
     globals

This split lets Nimbus reuse the high-level JavaScript context contract without
pretending that Deno ops are the generic host-call ABI.

## Permission Rules

Runtime permissions are independent of engine selection. Selecting a Node or
Bun-compatible JavaScript surface must not imply filesystem, network,
environment, subprocess, worker, FFI, tool, secret, service, or identity access.

Every engine must prove how `RuntimeGrants` are enforced before it is promoted
past an experimental backend:

- Deno/V8 may continue using Deno permission containers plus Nimbus host-call
  checks where appropriate.
- Bun/JSC must either patch or wrap Bun's filesystem, network, environment,
  subprocess, worker, and native surfaces so they consult Nimbus policy, or it
  must be restricted to explicitly trusted/sandboxed workloads until an
  equivalent policy proof exists.
- wasmtime must bind host imports through typed capability adapters and Store
  resource limits.

Engine defaults are not security policy. Nimbus grants are the policy.

## Compatibility Rules

Compatibility target and engine are separate choices.

- `Node20`, `Node22`, and `Node24` currently mean the measured Nimbus
  Node-compatible API surface implemented through Deno/V8.
- A future Bun-backed target should be explicit. Do not silently treat Bun as
  `Node22` only because Bun implements a Node-compatible API surface.
- A future wasmtime backend is not a JavaScript compatibility target. It is a
  different guest ABI and should not use JavaScript runtime target names.

Validation should reject unsupported combinations at policy construction time,
not fail later inside invocation execution.

## Promotion Gates For New Engines

Any new engine must satisfy these gates before becoming a selectable runtime
backend:

1. A local embed proof constructs the VM without process-owned CLI entrypoints.
2. The engine can install at least one sync and one async host call through
   `HostBridge`.
3. The engine can load a Nimbus bundle, invoke the exported entrypoint, settle
   promises or guest calls, and return a JSON-compatible value.
4. Timeout and external cancellation work and define whether the runtime is
   reusable after termination.
5. Memory pressure behavior is explicit, even if the first safe policy is
   discard-on-pressure.
6. Permission enforcement covers every host-sensitive builtin exposed by the
   compatibility target, or the backend is restricted to trusted/sandboxed
   workloads.
7. Reuse semantics are proven, or the backend starts as fresh-per-invocation.
8. Evidence harnesses record which compatibility claims are supported.
9. Generated artifacts, bundle metadata, server registry routing, and operator
   metadata all name the engine and compatibility target explicitly.
10. Link/build behavior is proven without process-global side effects leaking
   from an engine's CLI entrypoint into Nimbus.

These gates are intentionally stricter than "it links and can evaluate
JavaScript." A runtime engine is part of Nimbus's trust boundary.

For Bun/JSC specifically, the embed proof must avoid Bun's process-owning CLI
path, global allocator entrypoint, crash/signal setup, and process exit path.
It must also prove how required Bun codegen/native artifacts are produced or
consumed by Cargo without making Nimbus depend on an ad hoc local build
directory.

The concrete proof-harness layout and required evidence are defined in
[New runtime engine proof harness](new-engine-proof-harness.md). A new engine
cannot be promoted by adding a manifest selector alone; the proof must first
show build/link reproducibility, host-call transport, bundle loading,
cancellation, permission behavior, reuse or fresh-VM semantics, and safe
teardown.

## Current Refactor Target

The current Deno/V8 code should be treated as the first engine implementation,
not as the generic runtime abstraction. Step 0 for runtime extensions is:

- keep `WorkerLoop` as the scheduler-facing seam
- keep `HostBridge` as the Rust host ABI
- move Deno/V8 state ownership below a Deno/V8 engine/backend boundary
- extract the engine-neutral JavaScript context contract away from Deno ops
- make runtime policy validation describe supported engine, compatibility,
  execution, and pooling combinations
- separate bundle/content metadata from V8 module-specifier and code-cache
  state
- update generated artifacts and server lane selection before adding a second
  selectable JavaScript engine

Once that is true, Bun/JSC or wasmtime work can start as isolated backend
proofs instead of changing the existing V8 path while discovering the seam.
