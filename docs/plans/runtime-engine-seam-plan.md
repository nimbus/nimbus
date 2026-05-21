# Plan: Runtime Engine Seam

Canonical active plan for defining and hardening the runtime extension seam
before adding new in-process runtime engines such as Bun/JSC or wasmtime.

This plan is intentionally Step 0. It does not add Bun, JSC, wasmtime, or a new
Cargo dependency. It makes the current Deno/V8 path an explicit engine
implementation and creates the control plane for future runtime-extension work.

---

## Status

- **Status:** `active`
- **Primary owner:** this plan
- **Architecture reference:** `docs/architecture/runtime/engine-seam.md`
- **Autonomous goal prompt:** `docs/plans/prompts/runtime-engine-seam-goal.md`
- **Activation gate:** active now because future Node-compatible runtime,
  Bun/JSC, and wasmtime work all need a backend-neutral seam before
  implementation starts.

## Control Plan Rules

Use this plan before starting work that changes runtime engine ownership,
runtime backend selection, JavaScript bootstrap transport, or new runtime
engine proofs.

The source of truth is:

1. the current git worktree
2. this plan's phase ledger and execution log
3. `docs/architecture/runtime/engine-seam.md`
4. `ARCHITECTURE.md` for the landed `WorkerLoopFactory` / `WorkerLoop` seam
5. archived runtime plans only as historical context

Do not use prior chat transcripts as progress state. If a phase is
`in_progress`, resume it before opening a new phase. Record verification before
marking a phase `done`.

### Autonomous Resume Protocol

Every autonomous run must start by reconciling the plan with local git history:

1. Read this plan, `docs/architecture/runtime/engine-seam.md`,
   `ARCHITECTURE.md`, and `docs/plans/README.md`.
2. Run `git status --short`.
3. Inspect any dirty files before editing. Treat existing changes as user or
   prior-agent work and do not revert them unless explicitly asked.
4. Review the `Phase Status Ledger` and `Execution Log`.
5. If any phase is `in_progress`, resume that phase. Otherwise start the first
   `todo` phase whose dependencies are satisfied.
6. Before stopping, update this plan so the next run can resume from the plan
   plus git state alone.

Each stop point must leave one of these durable states:

- a completed phase marked `done` with verification recorded
- an active phase marked `in_progress` with exact next action recorded
- a blocked phase marked `blocked` with the blocker and required decision
  recorded

Never rely on chat history, local memory, or unstated context to carry progress
between runs.

### Checkpoint Format

When starting, completing, blocking, or splitting a phase, add or update an
`Execution Log` row with:

- date
- phase
- status
- files or symbols touched
- what changed
- verification command output summary
- exact next action

If the implementation requires splitting a phase, update the `Phase Status
Ledger` before making the code change. Keep at most one phase `in_progress` at
a time.

### Definition Of Done

This plan is complete only when all of the following are true:

- RS1 through RS6 are marked `done`.
- Deno/V8 remains the default runtime path and its existing behavior is
  verified after each code-changing phase.
- Worker-loop scheduling remains above engine-specific VM ownership.
- Backend invocation and worker queue envelopes no longer treat a Deno/V8
  runtime object as the generic carrier for future engines.
- The JavaScript context contract is separable from Deno-op transport while the
  current generated bundles still invoke through `__nimbusCreateContext`.
- `RuntimeBackendKind`, compatibility target, execution model, pooling model,
  and bundle content kind are validated as explicit policy axes.
- Generated artifacts and server registry routing have an explicit
  engine/content-kind shape that keeps current default and Node lanes stable.
- The Bun/JSC or first new-engine proof harness requirements are documented
  with commands, expected outputs, and promotion/non-promotion outcomes.
- Every completed phase has verification recorded in the `Execution Log`.

### Verification Matrix

Use focused commands first, then widen only when the changed surface requires
it. Record exact commands and outcomes in the `Execution Log`.

| Phase | Minimum verification before `done` |
| --- | --- |
| RS1 | Source-review checklist recorded in this plan; `git diff --check` for docs touched. |
| RS2 | `cargo test -p nimbus-runtime host_call --lib`; focused runtime/bootstrap tests for ctx, nested calls, scheduler, services, env/process policy, runtime extension calls; `cargo fmt --all --check`. |
| RS3 | Focused run-to-completion and cooperative runtime/executor tests; V8 pool reuse tests if pool or queue code changes; `cargo fmt --all --check`. |
| RS4 | Focused `nimbus-runtime` policy tests covering accepted and rejected engine/target/execution/pool/content combinations; `cargo fmt --all --check`. |
| RS5 | `npm run test --workspace @nimbus/codegen`; focused `nimbus-server` registry tests for lane selection and unsupported combinations; `cargo fmt --all --check` if Rust server code changes. |
| RS6 | Proof-harness command transcript recorded in the plan; proof must show link/build, sync host call, async host call, invoke, timeout/cancel, teardown, and explicit promotion decision. |

Before opening a PR or handing off a code-changing completed plan, run the
repo-standard broader checks appropriate to the touched surfaces, usually:
`cargo fmt --all --check`, focused `cargo test`, `npm run test --workspace
@nimbus/codegen` for codegen changes, and `make clippy` for shared Rust seam
changes.

## Why This Plan Exists

The current runtime code has a worker-local `RuntimeBackend` trait, but the
invocation path below it still assumes the Deno/V8 implementation. That is fine
for the current runtime family and dangerous as the extension seam for Bun/JSC
or wasmtime.

This plan creates a named path to:

- keep scheduling and admission above runtime engines
- keep `HostBridge` as the stable host ABI
- split engine-neutral Nimbus bootstrap behavior from Deno-op transport
- make runtime engine, compatibility target, execution model, and pooling model
  separate policy axes
- add future engines through proof-gated backend implementations instead of
  threading new VM types through V8-shaped code

## Phase Status Ledger

| Phase | Status | Goal | Verification |
| --- | --- | --- | --- |
| RS0 | `done` | Document the engine seam, axes, promotion gates, and Step 0 refactor target. | Documentation review; no code execution required. |
| RS1 | `done` | Audit Deno/V8-specific ownership across runtime, bundle, server registry, and codegen. | Source review with file/path checklist and no behavior changes. |
| RS2 | `done` | Split JavaScript bootstrap into engine-neutral Nimbus context plus Deno/V8 host-call transport. | Focused runtime tests for Convex ctx, nested calls, scheduler, services, env/process policy, runtime extension calls. |
| RS3 | `done` | Refactor backend invocation so worker loops pass an engine-neutral envelope and backends own engine state. | Existing V8 run-to-completion and cooperative runtime tests remain green. |
| RS4 | `done` | Add runtime policy validation for engine, compatibility target, execution model, pooling, and bundle content combinations. | Unit tests reject unsupported combinations at construction. |
| RS5 | `done` | Define artifact/codegen/server routing changes needed for selectable non-Deno/V8 engines. | Generated manifest tests prove engine metadata is explicit and current Node lanes remain unchanged. |
| RS6 | `done` | Define the isolated proof harness shape for new engines, starting with Bun/JSC if still desired. | Local proof can link, install host calls, invoke, cancel, and tear down without changing production defaults. |

## Invariants

- `WorkerLoopFactory` / `WorkerLoop` remain the scheduler-facing seam.
- `RuntimeBackendFactory` / `RuntimeBackend` remain worker-local helpers below
  the worker loop, not the top-level scheduling abstraction.
- `HostBridge` remains the Rust host ABI for runtime-to-server integration.
- `nimbus-runtime` keeps zero workspace dependencies.
- Deno/V8 setup stays owned by the Deno/V8 backend or engine implementation.
- New engines start behind explicit policy selection and proof gates.
- Engine selection never implies extra host permissions.
- Runtime bundle identity and cache keys must include content and engine
  configuration, not only a path-shaped entrypoint.
- Generated artifacts must name engine/content choices explicitly instead of
  overloading Node-only fields.
- Compatibility claims require evidence; runtime names are not blanket support
  claims.

## Phase Details

### RS0: Document The Seam

Status: `done`

Deliverables:

- add `docs/architecture/runtime/engine-seam.md`
- link the seam from runtime and architecture indexes
- promote this active plan as the owner for follow-on runtime-extension work

Acceptance criteria:

- the doc distinguishes engine, compatibility target, execution model, pooling
  model, and permission policy
- the doc names which responsibilities belong below the engine boundary
- the doc names generated artifact, bundle identity, server routing, and
  host-call ABI as part of the extension seam
- the doc states Bun/JSC and wasmtime promotion gates without committing to
  either implementation

### RS1: Audit Current Deno/V8 Ownership

Status: `done`

Deliverables:

- inventory Deno/V8-specific state in runtime construction, bootstrap, module
  loading, invocation, cancellation, pooling, and cooperative scheduling
- inventory V8-shaped assumptions in `RuntimeBundle`, module code cache,
  bundle integrity, and generated runtime preambles
- inventory server registry lane selection and manifest fields that currently
  model only `"default"` and `"node"` runtime environments
- inventory provider-neutral runtime extension calls and adapter-owned
  extension namespaces that must survive alternate engine transports
- identify which items stay Deno/V8-owned and which become engine-neutral
- update this plan with the smallest safe RS2-RS5 file list

Acceptance criteria:

- no code behavior changes
- no new backend dependencies
- audit names concrete files and symbols

#### RS1 Source Review Checklist

Runtime construction, invocation, cancellation, and V8 ownership:

- `crates/nimbus-runtime/src/backends/mod.rs`:
  `RuntimeBackendInvocation` currently carries `runtime: NimbusRuntime`, so
  the backend envelope hides a Deno/V8-capable facade as the generic carrier.
- `crates/nimbus-runtime/src/backends/v8/mod.rs`:
  `V8RuntimeBackendFactory`, `V8RuntimeBackend`, and
  `DeferredV8RuntimeDropQueue` are the only implemented backend path.
  `V8RuntimeBackend::invoke` calls
  `NimbusRuntime::invoke_bundle_unmanaged` and owns deferred `JsRuntime`
  drops.
- `crates/nimbus-runtime/src/worker_loop/run_to_completion.rs`:
  `RunToCompletionWorkerLoopFactory::new` hardcodes
  `V8RuntimeBackendFactory`, then rebuilds the backend invocation envelope
  with `job.runtime.clone().into_policy(...)`.
- `crates/nimbus-runtime/src/worker_loop/cooperative.rs` and
  `crates/nimbus-runtime/src/worker_loop/cooperative/*.rs`:
  cooperative scheduling is V8-specific despite its generic placement.
  `CooperativeWorkerLoop` owns `V8WorkerRuntimePool`,
  `CooperativeScheduler<CooperativeInvocation>`,
  `DeferredV8RuntimeDropQueue`, and `CooperativeInvocation::slot:
  CooperativeLockerRuntimeSlot`.
- `crates/nimbus-runtime/src/worker_loop/mod.rs`:
  `create_worker_loop_factory` selects by `RuntimeExecutionModel` only, not
  by `RuntimeBackendKind`.
- `crates/nimbus-runtime/src/executor/queue/job.rs`:
  `RuntimeWorkerJob` carries `runtime: NimbusRuntime`, so the executor queue
  also treats the facade as the hidden engine carrier.
- `crates/nimbus-runtime/src/runtime/facade.rs`:
  `NimbusRuntime` owns `host` and `policy` and remains the public runtime
  facade, but its internal use as a backend payload needs to become explicit
  host/policy/request data.
- `crates/nimbus-runtime/src/runtime/driver/construction.rs`:
  runtime construction is Deno/V8-specific through `deno_core::JsRuntime`,
  `RuntimeOptions`, startup snapshots, Deno extensions,
  `RestrictedModuleLoader`, V8 isolate limits, SharedArrayBuffer stores,
  inspector setup for Node targets, and `OpState` bootstrap repair.
- `crates/nimbus-runtime/src/runtime/driver/invocation.rs`:
  `RuntimeInvocationDriver` owns `JsRuntime`, V8 construction mode, timeout
  and cancellation handles, heap-limit termination, bundle integrity checks,
  `V8WorkerRuntimePool` checkout/return, and safe runtime reuse decisions.
- `crates/nimbus-runtime/src/runtime/driver/loading.rs`:
  module loading and execution use Deno/V8 APIs including
  `load_main_es_module`, `mod_evaluate`, `run_event_loop`, V8 code-cache
  deserialization, and scripted `globalThis.__nimbusInvoke(...)` entry.

Bootstrap, host-call transport, runtime-extension transport, and capability
state:

- `crates/nimbus-runtime/src/runtime/bootstrap/source.rs`:
  the high-level Nimbus/Convex JavaScript context contract is embedded in the
  same source string that binds `Deno.core.ops` to
  `__nimbusSyncHostValue`, `__nimbusAsyncHostValue`, and
  `__nimbusCreateContext`.
- `crates/nimbus-runtime/src/runtime/bootstrap/ops.rs` and
  `crates/nimbus-runtime/src/runtime/bootstrap/ops/*.rs`:
  Deno `extension!` registration owns all host-op transport, including
  database, scheduler, storage, search, HTTP, worker-thread, and
  `op_nimbus_runtime_extension_call` paths.
- `crates/nimbus-runtime/src/runtime/bootstrap/state.rs`:
  Deno `OpState` stores `InstalledRuntimeOwner`, `InstalledRuntimeHostBridge`,
  `InstalledRuntimeContract`, `InstalledRuntimeCapabilityPolicy`,
  `RuntimeCancellationState`, `SharedInvocationPermit`,
  `deno_permissions::PermissionsContainer`, and Deno web message-port state.
- `crates/nimbus-runtime/src/runtime_capabilities.rs`:
  runtime path/env/process policies are currently enforced with
  `deno_permissions` descriptor types and grants, even though permission
  decisions must stay policy-owned and not become engine selection side
  effects.
- `crates/nimbus-runtime/src/host.rs`:
  `HostBridge`, `HostCallOperation`, `HostCallPayload`, `HostCallEnvelope`,
  ABI version checks, and `RuntimeExtensionCall` are provider-neutral and must
  remain above any Deno-op, JSC host-function, or WASI import transport.
- `crates/nimbus-server/src/adapters/cloud_functions/host_bridge.rs`,
  `crates/nimbus-server/src/adapters/cloud_functions/runtime_api/extension.rs`,
  and `crates/nimbus-server/src/adapters/convex/host_bridge/**/*.rs`:
  adapter-owned `RuntimeExtensionCall` namespaces and dispatch must survive
  alternate engine transports without moving provider semantics into
  `nimbus-runtime`.

Bundle, module loader, and generated-artifact assumptions:

- `crates/nimbus-runtime/src/runtime/bundle.rs`:
  `RuntimeBundleShared` stores a Deno/V8-shaped `ModuleSpecifier`, a
  `BundleModuleCodeCache`, entrypoint paths, canonical module root, and
  expected SHA. It has no content-kind field and no engine-config component in
  cache identity.
- `crates/nimbus-runtime/src/module_loader.rs`:
  `RestrictedModuleLoader` is a Deno/V8 module loader that performs file
  loading, Node builtin resolution, package resolution, CommonJS translation,
  and code-cache lookups.
- `crates/nimbus-runtime/src/node_compat.rs`:
  Node compatibility is implemented through Deno-family resolver and CommonJS
  loader glue. This stays with the Deno/V8 JavaScript engine family unless a
  new engine proves equivalent semantics explicitly.
- `packages/codegen/src/emit/runtime_bundle_preamble.mjs` and
  `packages/codegen/src/emit/runtime_bundle.mjs`:
  generated bundles assume `globalThis.__nimbusCreateContext`, runtime-handler
  `new Function` dispatch, and Node import maps only for Node-compatible
  functions.

Policy, server registry, and manifest shape:

- `crates/nimbus-runtime/src/limits.rs`:
  `RuntimeBackendKind` currently has only `V8`; compatibility targets are
  `WebStandardIsolate`, `Node20`, `Node22`, and `Node24`; execution models are
  `RunToCompletion` and `CooperativeLocker`; pool kinds are
  `StartupSnapshotCache` and `WarmPool`. Normalization validates several
  target/model/pool relationships but does not yet validate content kind or use
  backend kind for backend selection.
- `packages/codegen/src/project_config.mjs`,
  `packages/codegen/src/parser.mjs`, `packages/codegen/src/main.mjs`, and
  `packages/codegen/src/node_external_packages.mjs`:
  generated manifests model runtime choice as `"default"` or `"node"` with
  `node_version`, `node_runtime_target`, and `node.externalPackages`. There is
  no explicit runtime engine or bundle content kind.
- `crates/nimbus-server/src/adapters/convex/manifest.rs`,
  `crates/nimbus-server/src/adapters/convex/mod.rs`, and
  `crates/nimbus-server/src/adapters/convex/registry/resolution/runtime_access.rs`:
  the Convex registry has default and Node20/Node22/Node24 lanes selected from
  `ConvexRuntimeEnvironment` plus `node_runtime_target`; lane selection does
  not yet key by engine/content kind.
- `crates/nimbus-server/src/protocol.rs` and
  `crates/nimbus-server/src/http/metadata.rs`:
  operator-facing metadata exposes the current runtime backend,
  compatibility target, and execution model. Future engine routing must keep
  these fields truthful rather than overloading Node target names.

Bun/JSC research hazards for RS6 proof planning:

- `/Users/jack/src/github.com/oven-sh/bun/src/bun_bin/lib.rs` owns Bun's
  process entry path, global allocator setup, crash/signal handling, stdio
  setup, parent-death watchdog, `bun_runtime::cli::Cli::start()`, and
  process exit. A Nimbus in-process proof must avoid this path.
- `/Users/jack/src/github.com/oven-sh/bun/src/bun_bin/Cargo.toml` builds a
  `staticlib` named `bun_rust`, not an embeddable runtime crate.
- `/Users/jack/src/github.com/oven-sh/bun/src/runtime/Cargo.toml` depends on
  many Bun subsystems, so an in-process proof must isolate the minimal JSC/VM
  surface rather than assuming `bun_runtime` is a small library boundary.
- `/Users/jack/src/github.com/oven-sh/bun/src/jsc/build.rs` and
  `/Users/jack/src/github.com/oven-sh/bun/src/runtime/generated_host_exports.rs`
  require generated code directories, so a proof must document reproducible
  generation/build commands and must not depend on untracked local state.
- `/Users/jack/src/github.com/oven-sh/bun/src/jsc/VirtualMachine.rs`,
  `/Users/jack/src/github.com/oven-sh/bun/src/jsc/VM.rs`,
  `/Users/jack/src/github.com/oven-sh/bun/src/jsc/JSFunction.rs`, and
  `/Users/jack/src/github.com/oven-sh/bun/src/jsc/JSModuleLoader.rs` expose
  VM setup, execution limits, host functions, and module loading hooks that
  may support a proof harness, but safe teardown, cancellation, permissions,
  and reuse are not established.

#### Ownership Decisions

Keep Deno/V8-owned:

- `deno_core::JsRuntime`, `RuntimeOptions`, V8 isolate handles, startup
  snapshots, Deno extensions, Deno `OpState`, V8 heap-limit callbacks,
  `V8WorkerRuntimePool`, `CooperativeLockerRuntimeSlot`, Deno module loading,
  Deno Node-compat resolver glue, and Deno permission descriptor plumbing.

Make engine-neutral:

- worker-to-backend invocation data, host bridge references, normalized runtime
  policy axes, bundle identity/content metadata, generated manifest engine
  metadata, server registry lane selection shape, host-call envelopes,
  `RuntimeExtensionCall`, cancellation intent, invocation context, metrics, and
  the JavaScript Nimbus context contract above transport-specific host-call
  primitives.

#### Smallest Safe RS2-RS5 File Lists

RS2 bootstrap contract split:

- `crates/nimbus-runtime/src/runtime/bootstrap/source.rs`
- `crates/nimbus-runtime/src/runtime/bootstrap/mod.rs`
- `crates/nimbus-runtime/src/runtime/bootstrap/ops.rs`
- `crates/nimbus-runtime/src/runtime/bootstrap/payloads.rs`
- focused runtime/bootstrap tests covering host calls, ctx creation, nested
  calls, scheduler, services, env/process policy, and runtime-extension calls
- `packages/codegen/src/emit/runtime_bundle_preamble.mjs` only if the split
  requires a generated preamble shape change; the preferred RS2 path preserves
  the existing `__nimbusCreateContext` call site

RS3 backend invocation envelope:

- `crates/nimbus-runtime/src/backends/mod.rs`
- `crates/nimbus-runtime/src/backends/v8/mod.rs`
- `crates/nimbus-runtime/src/worker_loop/run_to_completion.rs`
- `crates/nimbus-runtime/src/worker_loop/cooperative.rs`
- `crates/nimbus-runtime/src/worker_loop/cooperative/*.rs`
- `crates/nimbus-runtime/src/executor/queue/job.rs`
- `crates/nimbus-runtime/src/runtime/facade.rs`
- focused executor, run-to-completion, cooperative, and V8 pool reuse tests

RS4 runtime policy validation:

- `crates/nimbus-runtime/src/limits.rs`
- `crates/nimbus-runtime/src/worker_loop/mod.rs`
- `crates/nimbus-runtime/src/runtime/bundle.rs`
- `crates/nimbus-runtime/src/lib.rs` if new public policy/content-kind types
  need re-exporting
- focused policy tests for accepted and rejected engine/target/model/pool and
  content-kind combinations

RS5 artifact, codegen, and server routing shape:

- `packages/codegen/src/project_config.mjs`
- `packages/codegen/src/parser.mjs`
- `packages/codegen/src/main.mjs`
- `packages/codegen/src/emit/runtime_bundle.mjs`
- `packages/codegen/src/emit/runtime_bundle_preamble.mjs`
- `packages/codegen/src/node_external_packages.mjs`
- `packages/codegen/src/selftest/action_fixtures.mjs`
- `crates/nimbus-server/src/adapters/convex/manifest.rs`
- `crates/nimbus-server/src/adapters/convex/mod.rs`
- `crates/nimbus-server/src/adapters/convex/registry/loading.rs`
- `crates/nimbus-server/src/adapters/convex/registry/resolution/runtime_access.rs`
- `crates/nimbus-server/src/protocol.rs`
- `crates/nimbus-server/src/http/metadata.rs`
- focused codegen selftests and server registry tests

### RS2: Split Bootstrap Contract From Transport

Status: `done`

Deliverables:

- extract the engine-neutral JavaScript context contract from the current
  Deno-op-backed bootstrap source
- inject sync and async host-call primitives from the Deno/V8 transport layer
- carry `RuntimeExtensionCall` through the same injected host-call primitive
  path without moving adapter-owned namespaces into `nimbus-runtime`
- preserve `SharedInvocationPermit` suspend/resume and timeout-pause behavior
  around async host calls
- keep generated bundles' observable Nimbus/Convex behavior unchanged

Acceptance criteria:

- current Deno/V8 runtime tests stay green
- generated bundle preamble still reaches `__nimbusCreateContext`
- Deno ops are no longer the only place where the high-level context contract
  can be installed
- host-call ABI tests still reject mismatched operation/payload pairs and
  unsupported ABI versions

### RS3: Backend Invocation Envelope

Status: `done`

Deliverables:

- make the worker-to-backend invocation envelope engine-neutral
- remove `NimbusRuntime` as the hidden Deno/V8 carrier from backend
  invocation and worker queue internals where practical, replacing it with
  explicit host bridge, policy, bundle, request, context, and cancellation
  data
- move Deno/V8 runtime pool and VM handles fully below the Deno/V8 backend
- keep cooperative V8-specific scheduling in a named Deno/V8 driver instead of
  generic worker-loop state

Acceptance criteria:

- V8 run-to-completion behavior is unchanged
- cooperative V8 behavior is unchanged
- no Bun/JSC or wasmtime code is introduced in this phase

### RS4: Runtime Policy Validation

Status: `done`

Deliverables:

- validate backend engine, compatibility target, execution model, and pooling
  combinations when runtime policy is constructed
- validate bundle content kind against backend engine and compatibility target
- make `RuntimeBackendKind` actually participate in worker-loop/backend
  factory selection or reject non-default values until selection exists
- keep current V8 defaults stable
- reject future-only combinations clearly

Acceptance criteria:

- focused policy tests cover accepted and rejected combinations
- unsupported combinations fail before invocation execution starts

### RS5: Artifact, Codegen, And Server Routing Shape

Status: `done`

Deliverables:

- add a proposed manifest shape for engine/content-kind metadata that keeps
  current `"use node"` behavior stable
- define how external packages are resolved for each JavaScript engine instead
  of assuming Node/Deno resolution applies to Bun/JSC
- define server registry lane selection by engine plus compatibility target,
  not only by `node_runtime_target`
- define operator/metadata API fields for engine and compatibility target
- map bundle cache keys to content hash plus engine configuration

Acceptance criteria:

- codegen selftests cover the current default and Node20/Node22/Node24 lanes
  unchanged
- new manifest fields are explicit enough that Bun/JSC does not masquerade as
  Node22
- server routing rejects unsupported engine/target combinations before
  invocation

### RS6: New Engine Proof Harness Shape

Status: `done`

Deliverables:

- define a non-production proof harness for adding an engine backend
- if Bun/JSC is first, prove VM construction below Bun's CLI path, host
  functions, async host calls, bundle load, timeout/cancel, and teardown
- if Bun/JSC is first, prove link/build behavior without depending on
  `bun_bin`, Bun's global allocator entrypoint, crash/signal installation,
  process exit path, or an untracked local codegen directory
- if wasmtime is first, prove component load, typed imports, fuel/epoch
  interruption, Store lifecycle, and resource limits

Acceptance criteria:

- proof code is isolated from production defaults
- promotion gates from `docs/architecture/runtime/engine-seam.md` are mapped to
  concrete checks
- failure to prove permissions or safe reuse keeps the backend experimental

Outcome:

- `docs/architecture/runtime/new-engine-proof-harness.md` now defines the
  non-production proof location, feature/ignored-test isolation model,
  required evidence table, Bun/JSC-specific forbidden process-owned paths,
  wasmtime-specific component checks, and promotion decision gate.
- `docs/architecture/runtime/engine-seam.md` links the proof harness as the
  concrete evidence contract for the existing promotion gates.
- Bun/JSC remains proof-only until a local harness can prove reproducible
  build/link behavior, host-call transport, bundle loading, timeout/cancel,
  permission behavior, safe teardown, and reuse or fresh-VM semantics.

## Execution Log

| Date | Phase | Status | Notes | Verification |
| --- | --- | --- | --- | --- |
| 2026-05-21 | RS0 | `done` | Added the active runtime engine seam reference and this control plan so Bun/JSC or wasmtime work has a Step 0 boundary before implementation. | Documentation-only change; no runtime verification required. |
| 2026-05-21 | audit | `done` | Audited the live runtime/codegen/server seams plus the existing wasmtime and Locker research. Tightened the seam docs to include generated artifact metadata, bundle content/cache keys, runtime-extension host-call ABI parity, server lane selection, and Bun/JSC build/link hazards. | Source/docs review only; no runtime verification required. |
| 2026-05-21 | RS1 | `in_progress` | Goal activated from `docs/plans/prompts/runtime-engine-seam-goal.md`. Startup protocol read `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`, and inspected the dirty worktree. Next action: complete the RS1 source-review checklist by recording concrete runtime, bundle, codegen, server-registry, and runtime-extension symbols/files in this plan before marking RS1 `done`. | `git status --short` reviewed; no code verification yet because this is the startup checkpoint. |
| 2026-05-21 | RS1 | `done` | Recorded the concrete Deno/V8 ownership audit, engine-neutral ownership decisions, Bun/JSC proof hazards, and the smallest safe RS2-RS5 file lists. Next action: start RS2 by splitting bootstrap source into transport-specific Deno host-call installation plus the engine-neutral Nimbus context contract. | `git diff --check -- docs/plans/runtime-engine-seam-plan.md` passed with no whitespace errors. |
| 2026-05-21 | RS2 | `in_progress` | Read `source.rs`, bootstrap module exports, Deno op registration, payload aliases, host-call references, runtime tests, Convex AI guidelines, and server auth/runtime trust docs. Editing target is `crates/nimbus-runtime/src/runtime/bootstrap/source.rs`: split the monolithic bootstrap source into Deno host-call transport, engine-neutral Nimbus context contract, and Deno runtime-globals source while preserving install order and generated `__nimbusCreateContext` behavior. | Source review only so far; next action is code split plus focused `nimbus-runtime` tests and `cargo fmt --all --check`. |
| 2026-05-21 | RS2 | `done` | Split `crates/nimbus-runtime/src/runtime/bootstrap/source.rs` into `DENO_HOST_CALL_TRANSPORT_SOURCE`, `NIMBUS_CONTEXT_CONTRACT_SOURCE`, and `DENO_RUNTIME_GLOBALS_SOURCE`; updated the V8 startup snapshot comment in `crates/nimbus-runtime/src/backends/v8/startup.rs`; added a direct runtime-extension transport test in `crates/nimbus-runtime/src/runtime/tests/host_bridge.rs`. Generated bundle call sites still use `globalThis.__nimbusCreateContext`. Next action: start RS3 by making the worker-to-backend invocation envelope explicit and engine-neutral while keeping Deno/V8 as the only implementation. | `cargo test -p nimbus-runtime bootstrap::source --lib` passed 3 tests; `cargo test -p nimbus-runtime host_call --lib` passed 6 tests; `cargo test -p nimbus-runtime runtime::tests::host_bridge --lib` passed 14 tests; `cargo test -p nimbus-runtime runtime::tests::basic_invocation::web_standard --lib` passed 8 tests; `cargo test -p nimbus-runtime runtime::tests::basic_invocation::node_capabilities --lib` passed 6 tests; `cargo test -p nimbus-runtime runtime::tests::basic_invocation::node_bootstrap --lib` passed 9 tests; `cargo test -p nimbus-runtime runtime::tests::snapshot_lifecycle --lib` passed 4 tests; `cargo test -p nimbus-runtime runtime_builds_locker_jsruntime_from_snapshot_subprocess --lib -- --ignored` passed 1 test; `cargo fmt --all --check` passed; `git diff --check -- crates/nimbus-runtime/src/runtime/bootstrap/source.rs crates/nimbus-runtime/src/backends/v8/startup.rs crates/nimbus-runtime/src/runtime/tests/host_bridge.rs docs/plans/runtime-engine-seam-plan.md` passed. |
| 2026-05-21 | RS3 | `in_progress` | Read backend, worker-loop, executor queue, runtime facade, cooperative runtime slot, V8 warm-pool, and queue-router test files. Planned code shape: add a crate-internal engine-neutral host handle, replace `RuntimeWorkerJob::runtime` and `RuntimeBackendInvocation::runtime` with that handle plus explicit policy, and reconstruct the Deno/V8 owner only below the V8 backend/cooperative V8 driver. | Source review only so far; next action is code refactor plus focused run-to-completion, cooperative, and pool-reuse verification. |
| 2026-05-21 | RS3 | `done` | Added crate-internal `RuntimeHost`; removed `NimbusRuntime` from `RuntimeWorkerJob` and `RuntimeBackendInvocation`; made backend invocations carry explicit `RuntimePolicy`; reconstructed the current Deno/V8 owner only inside the V8 backend and cooperative V8 path; updated queue-router tests for the new host handle. No Bun/JSC or wasmtime production code was introduced. Next action: start RS4 by adding explicit runtime policy validation for engine, compatibility target, execution model, pooling, and bundle content combinations. | `cargo check -p nimbus-runtime` passed; `cargo test -p nimbus-runtime runtime::tests::basic_invocation::web_standard --lib` passed 8 tests; `cargo test -p nimbus-runtime runtime::tests::host_bridge --lib` passed 14 tests; `cargo test -p nimbus-runtime executor::tests::lifecycle --lib` passed 5 tests; `cargo test -p nimbus-runtime executor::tests::cooperative --lib` passed 3 tests; `cargo test -p nimbus-runtime runtime::tests::pool_reuse --lib` passed 5 tests; `cargo test -p nimbus-runtime runtime::tests::cooperative --lib` passed 4 tests with 4 ignored subprocess tests; `cargo test -p nimbus-runtime runtime::tests::cooperative --lib -- --ignored` passed 4 subprocess tests; `cargo fmt --all --check` passed; `git diff --check -- crates/nimbus-runtime/src/runtime.rs crates/nimbus-runtime/src/runtime/facade.rs crates/nimbus-runtime/src/backends/mod.rs crates/nimbus-runtime/src/backends/v8/mod.rs crates/nimbus-runtime/src/executor/invoke.rs crates/nimbus-runtime/src/executor/queue/job.rs crates/nimbus-runtime/src/executor/queue/router.rs crates/nimbus-runtime/src/worker_loop/run_to_completion.rs crates/nimbus-runtime/src/worker_loop/cooperative/execution.rs crates/nimbus-runtime/src/worker_loop/cooperative/run.rs crates/nimbus-runtime/src/worker_loop/cooperative/retention.rs docs/plans/runtime-engine-seam-plan.md` passed; `rg` found no `RuntimeWorkerJob` or `RuntimeBackendInvocation` runtime field. |
| 2026-05-21 | RS4 | `done` | Added `RuntimeBundleContentKind` as an explicit runtime policy and bundle identity axis; defaulted current bundles and policies to JavaScript; rejected V8 plus non-JavaScript bundle content during `RuntimePolicy::new`; added invocation-time policy/bundle content-kind validation before V8 execution; kept current V8/WebStandard/Node20/Node22/Node24 combinations stable. Next action: start RS5 by making generated artifacts and server routing expose explicit engine/content metadata without changing current default and Node lanes. | `cargo test -p nimbus-runtime limits::tests --lib` passed 8 tests; `cargo test -p nimbus-runtime runtime::tests::basic_invocation::web_standard --lib` passed 8 tests; `cargo test -p nimbus-runtime runtime::tests::host_bridge --lib` passed 14 tests; `cargo test -p nimbus-runtime executor::tests::cooperative --lib` passed 3 tests; `cargo check -p nimbus-runtime` passed; `cargo fmt --all --check` passed; `git diff --check -- crates/nimbus-runtime/src/limits.rs crates/nimbus-runtime/src/runtime/bundle.rs crates/nimbus-runtime/src/lib.rs crates/nimbus-runtime/src/runtime/driver/invocation.rs crates/nimbus-runtime/src/runtime/cooperative.rs docs/plans/runtime-engine-seam-plan.md` passed. |
| 2026-05-21 | RS5 | `done` | Added generated `runtime_engine`, `runtime_bundle_content_kind`, `runtime_compatibility_target`, and `runtime_package_resolution` metadata per Convex function plus top-level `runtime_lanes`; kept `"use node"` selecting Node20/Node22/Node24 from `convex.json`; made the Convex registry parse and validate runtime engine/content/target/package-resolution combinations before invocation; routed runtime lanes by explicit V8 engine plus compatibility target; exposed `bundle_content_kind` in runtime diagnostics; partitioned V8 module code caches by engine/content/target so bundle cache reuse follows runtime configuration. Next action: start RS6 by writing the isolated proof-harness contract and Bun/JSC decision gate without adding a production backend. | `npm run test --workspace @nimbus/codegen` passed; `cargo test -p nimbus-server registry_and_license::registry --lib` passed 7 tests; `cargo test -p nimbus-runtime runtime::tests::bundle_integrity --lib` passed 9 tests with 1 ignored subprocess test; `cargo check -p nimbus-server` passed; `cargo fmt --all --check` passed; `git diff --check -- packages/codegen/src/runtime_metadata.mjs packages/codegen/src/main.mjs packages/codegen/src/selftest/runtime_metadata_assertions.mjs packages/codegen/src/selftest/runtime_fixtures.mjs packages/codegen/src/selftest/action_fixtures.mjs crates/nimbus-runtime/src/limits.rs crates/nimbus-runtime/src/runtime/bundle.rs crates/nimbus-runtime/src/runtime/driver/construction.rs crates/nimbus-runtime/src/runtime/tests/bundle_integrity.rs crates/nimbus-server/src/adapters/convex/manifest.rs crates/nimbus-server/src/adapters/convex/registry/loading.rs crates/nimbus-server/src/adapters/convex/registry/resolution/runtime_access.rs crates/nimbus-server/src/protocol.rs crates/nimbus-server/src/http/metadata.rs crates/nimbus-server/src/tests/registry_and_license/registry.rs docs/plans/runtime-engine-seam-plan.md` passed. |
| 2026-05-21 | RS6 | `done` | Added the non-production new-engine proof harness contract and linked it from the runtime engine seam. The contract maps promotion gates to concrete build/link, VM construction, sync/async host call, bundle load, runtime-extension, timeout/cancel, memory, permission, reuse/teardown, artifact metadata, and server routing evidence. Bun/JSC remains blocked from production selection unless it avoids Bun's CLI/process-owned path and proves reproducible generated/native artifacts, host functions, cancellation, permissions, teardown, and reuse or fresh-invocation semantics. | Documentation-only proof-gate change; `git diff --check -- docs/architecture/runtime/new-engine-proof-harness.md docs/architecture/runtime/engine-seam.md docs/README.md docs/plans/runtime-engine-seam-plan.md` passed. |
| 2026-05-21 | Bun/JSC Gate 1 | `blocked` | Reproduced the next VM-construction probe against Bun `0b20408b656f95aa347cb6c06eb03c14a20051cb`. `cargo test -p bun_jsc --lib --no-run` fails before link on Bun's `-D dead-code` macro-smoke type; forcing `-Adead_code` reaches a wrong-shaped Cargo test link that fails on macOS with `ld: library 'ntdll' not found`. A Bun `--configure-only` full native graph exposes `bun-rust`/`libbun_rust.a`, `bun-debug`, `bun`, and `check`, but no smaller non-CLI VM-construction executable. Next action: identify or create an upstream Bun embeddable link target below `bun_bin` before writing a Nimbus VM-construction proof. | `bun scripts/build.ts --profile=debug --build-dir=/private/tmp/nimbus-bun-vm-link-probe --cache-dir=/private/tmp/nimbus-bun-cache --configure-only` passed; `rg` against `/private/tmp/nimbus-bun-vm-link-probe/build.ninja` found only the full CLI/native graph surfaces; `CARGO_TARGET_DIR=/private/tmp/nimbus-bun-proof-target BUN_CODEGEN_DIR=/private/tmp/nimbus-bun-rust-only/codegen CARGO_ENCODED_RUSTFLAGS= cargo test -p bun_jsc --lib --no-run` failed on `-D dead-code`; `CARGO_TARGET_DIR=/private/tmp/nimbus-bun-proof-target BUN_CODEGEN_DIR=/private/tmp/nimbus-bun-rust-only/codegen CARGO_ENCODED_RUSTFLAGS=-Adead_code cargo test -p bun_jsc --lib --no-run` failed at link on missing `ntdll`; Bun worktree stayed clean. |
| 2026-05-21 | Bun/JSC Gate 2 | `partial` | Built a temporary non-CLI Rust staticlib root outside both repos that depends on `bun_core`, `bun_jsc`, and `bun_runtime`, touches `bun_runtime::jsc_hooks::runtime_state()` so `__BUN_RUNTIME_HOOKS` is owned outside `bun_bin`, and compiles a C ABI function that calls `VirtualMachine::init` with `InitOptions::is_main_thread = false` then `VirtualMachine::destroy`. This proves `bun_bin` is not the only possible Rust archive root, but it still does not prove runnable VM construction because Cargo staticlib output does not final-link Bun's C++/WebKit/JSC graph. Next action: add or identify a Bun-side native target that links this kind of non-CLI Rust root against the same native inputs and executes the exported VM-construction function. | First out-of-worktree run selected stable Rust and failed on `bun_wyhash` nightly feature usage. With `RUSTUP_TOOLCHAIN=nightly-2026-05-06`, `CARGO_TARGET_DIR=/private/tmp/nimbus-bun-embed-probe-target BUN_CODEGEN_DIR=/private/tmp/nimbus-bun-rust-only/codegen CARGO_ENCODED_RUSTFLAGS= cargo check --manifest-path /private/tmp/nimbus-bun-embed-probe/Cargo.toml --lib` passed in 33.90s; matching `cargo build --manifest-path /private/tmp/nimbus-bun-embed-probe/Cargo.toml --lib` passed in 50.11s and produced `libnimbus_bun_embed_probe.a` (643M) plus `libnimbus_bun_embed_probe.rlib` (854K); `nm -gU ... | rg "_nimbus_bun_embed_probe_construct_and_destroy_vm"` found the exported symbol while Apple `nm` warned on Rust-nightly object metadata; Bun worktree stayed clean. |
