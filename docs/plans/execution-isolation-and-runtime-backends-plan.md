# Plan: Execution Isolation And Runtime Backends

Canonical active control plane for Nimbus execution boundaries after the
runtime engine seam baseline.

This plan owns the next phase where runtime engines, sandbox isolation,
capability policy, resource limits, admission, and artifact metadata meet. It
keeps Deno/V8 as the production default, keeps Bun/JSC proof-only until
promotion gates are satisfied, and gives wasmtime, WASI agent capabilities, and
krun sandbox hardening one shared execution-boundary map.

---

## Status

- **Status:** `active`
- **Primary owner:** this plan
- **Predecessor baseline:**
  `docs/plans/archive/runtime-engine-seam-plan.md`
- **Autonomous goal prompt:**
  `docs/plans/prompts/execution-isolation-runtime-backends-goal.md`
- **Architecture references:**
  - `docs/architecture/runtime/engine-seam.md`
  - `docs/architecture/runtime/new-engine-proof-harness.md`
  - `docs/architecture/runtime/permission-model.md`
  - `docs/architecture/sandbox/microvm-service-baseline.md`
  - `docs/plans/security/sandbox-isolation-audit.md`
- **Activation gate:** active now because the runtime engine seam work is
  complete and future Bun/JSC, wasmtime, WASI agent, sandbox, and admission
  work need one shared boundary plan before implementation continues.

## Control Plan Rules

Use this plan before starting work that changes any of these surfaces:

- runtime backend selection or promotion
- in-process runtime engine proof gates
- sandbox or microVM isolation posture
- execution capability grants or trust tiers
- runtime memory, timeout, cancellation, or VM reuse policy
- WASM/WASI component execution or agent capabilities
- admission/resource gates that protect execution or sandbox capacity
- generated artifact metadata for runtime/backend selection

The source of truth is:

1. the current git worktree
2. this plan's phase ledger and execution log
3. `docs/architecture/runtime/engine-seam.md`
4. `docs/architecture/runtime/new-engine-proof-harness.md`
5. `docs/plans/security/sandbox-isolation-audit.md`
6. archived baselines only as historical evidence

Do not use prior chat transcripts as progress state. If a phase is
`in_progress`, resume it before opening a new phase. Record verification before
marking a phase `done`.

### Autonomous Resume Protocol

Every autonomous run must start by reconciling the plan with local git history:

1. Read this plan, `README.md`, `ARCHITECTURE.md`, `docs/README.md`,
   `docs/plans/README.md`, `docs/architecture/runtime/engine-seam.md`, and
   `docs/architecture/runtime/new-engine-proof-harness.md`.
2. Run `git status --short`.
3. Inspect any dirty files before editing. Treat existing changes as user or
   prior-agent work and do not revert them unless explicitly asked.
4. Review the `Phase Status Ledger` and `Execution Log`.
5. If any phase is `in_progress`, resume that phase. Otherwise start the first
   `todo` phase whose dependencies are satisfied.
6. Before stopping, update this plan so the next run can resume from this plan
   plus git state alone.

Each stop point must leave one of these durable states:

- a completed phase marked `done` with verification recorded
- an active phase marked `in_progress` with exact next action recorded
- a blocked phase marked `blocked` with the blocker and required decision
  recorded

### Definition Of Done

This plan is complete only when all of the following are true:

- The runtime, sandbox, WASM/WASI, admission, and security-audit plans have a
  single ownership map that says which active or deferred plan owns each
  execution-boundary concern.
- Runtime backend promotion has explicit trust tiers, not only engine names.
- Bun/JSC has a recorded go/no-go decision for forking, permissions, memory,
  package loading, VM reuse, and production selection.
- Sandbox isolation findings are either assigned to implementation plans or
  documented as accepted residual risk with operator controls.
- Wasmtime and WASI agent plans depend on the same capability and trust-tier
  vocabulary as in-process runtimes and sandboxed services.
- Admission/resource gates protect concrete resources and name overload
  behavior before implementation starts.
- Active plans in `docs/plans/README.md` remain small and non-overlapping.
- Every completed phase has verification recorded in the `Execution Log`.

## Current Architectural Direction

Nimbus should model execution as layered boundaries, not as one generic
runtime switch:

```text
Server / adapters
  -> admission and scheduling
    -> execution backend or sandbox backend
      -> engine or guest process
        -> capability transport
          -> HostBridge / AgentOsProvider / sandbox endpoint policy
```

The runtime engine seam baseline answered the first in-process question:
`WorkerLoop` remains scheduler-facing, `RuntimeBackend` owns engine state, and
the Deno/V8 implementation no longer hides behind generic envelopes.

The next question is broader: which workloads are safe in-process, which need a
WASM capability sandbox, which require a microVM, and which are trusted-only?
That answer must combine runtime engine behavior, permission policy, resource
limits, sandbox isolation, and admission control.

## Bun/JSC Direction

Do not fork Bun as a product dependency yet.

The local Bun proof commits show that an embeddable Bun/JSC path is plausible:
the proof target links below `bun_bin`, constructs and destroys a VM, installs
sync and async host functions, executes a generated Nimbus program wrapper, and
proves host-owned timeout/cancel recovery.

The evidence is still not enough for production selection. Bun/JSC remains
proof-only until the following are answered:

- permission containment for filesystem, network, env, subprocess, worker, FFI,
  native addons, package loading, and Bun-specific APIs
- memory limit or discard-on-pressure policy
- ESM/module/package-resolution model
- VM reuse versus fresh-per-invocation lifecycle
- reproducible dependency and generated-artifact strategy
- server/codegen/operator metadata for a selectable backend

A fork becomes justified only if Nimbus chooses to ship Bun/JSC and upstream
Bun cannot or will not accept the embeddable target, link bridge, or policy
hooks needed for the production path. Until then, keep the local Bun worktree
as proof evidence and avoid creating a long-lived maintenance fork.

## Relationship To Existing Plans

| Plan or doc | Role under this plan |
| --- | --- |
| `docs/plans/archive/runtime-engine-seam-plan.md` | Completed Step 0 baseline for runtime backend seams and Bun/JSC gates 1-10. |
| `docs/architecture/runtime/engine-seam.md` | Stable runtime seam reference. |
| `docs/architecture/runtime/new-engine-proof-harness.md` | Required evidence table before a runtime backend can become selectable. |
| `docs/architecture/runtime/permission-model.md` | Shared trust-tier, mode, grant, preset, and compatibility-target policy. |
| `docs/plans/security/sandbox-isolation-audit.md` | Security findings that must be routed into sandbox hardening or accepted-risk decisions. |
| `docs/plans/wasmtime-backend-plan.md` | Deferred backend-specific wasmtime execution plan; consumes this plan's trust and capability vocabulary. |
| `docs/plans/wasi-agent-capabilities-plan.md` | Deferred agent capability plan; consumes this plan's capability and sandbox tiers. |
| `docs/plans/layered-admission-control-plan.md` | Deferred admission-control plan; consumes this plan's resource boundary map before adding gates. |
| `docs/plans/distribution-plan.md` | Owns packaging and distribution once execution/sandbox artifacts need shipping. |

## EIB1 Ownership Map

Status: `done` as of 2026-05-21.

This map is the Step 0 guardrail for implementation: no runtime, sandbox,
WASM/WASI, or admission change should start until the boundary it touches has
an owner below or this map has been updated.

### Source Review Checklist

Reviewed source and docs:

- `README.md`, `ARCHITECTURE.md`, `docs/README.md`,
  `docs/plans/README.md`
- `docs/architecture/runtime/engine-seam.md`
- `docs/architecture/runtime/new-engine-proof-harness.md`
- `docs/plans/security/sandbox-isolation-audit.md`
- `docs/plans/wasmtime-backend-plan.md`
- `docs/plans/wasi-agent-capabilities-plan.md`
- `docs/plans/layered-admission-control-plan.md`
- `crates/nimbus-runtime/src/backends/mod.rs`
- `crates/nimbus-runtime/src/worker_loop/mod.rs`
- `crates/nimbus-runtime/src/executor/admission/permit.rs`
- `crates/nimbus-runtime/src/limits.rs`
- `crates/nimbus-runtime/src/host.rs`
- `crates/nimbus-runtime/src/runtime/bootstrap/source.rs`
- `crates/nimbus-runtime/src/runtime/bundle.rs`
- `crates/nimbus-runtime/src/runtime_capabilities.rs`
- `crates/nimbus-runtime/src/metrics.rs`
- `crates/nimbus-sandbox/src/backend.rs`
- `crates/nimbus-sandbox/src/spec.rs`
- `crates/nimbus-sandbox/src/backends/krun/bundle.rs`
- `crates/nimbus-server/src/sandbox.rs`
- `crates/nimbus-server/src/adapters/convex/manifest.rs`
- `crates/nimbus-server/src/adapters/convex/registry/resolution/runtime_access.rs`
- `crates/nimbus-server/src/adapters/convex/host_bridge/contract.rs`
- `crates/nimbus-server/src/adapters/convex/host_bridge/bridge.rs`
- `crates/nimbus-engine/src/service/mutations/journal.rs`
- `crates/nimbus-storage/src/async_storage/traits.rs`
- `packages/codegen/src/runtime_metadata.mjs`
- `packages/codegen/src/main.mjs`

### Boundary Inventory

| Boundary | Concrete files and symbols | Owning plan or doc | Hand-off rule |
| --- | --- | --- | --- |
| Runtime scheduling and invocation admission | `crates/nimbus-runtime/src/worker_loop/mod.rs`, `worker_loop/run_to_completion.rs`, `worker_loop/cooperative.rs`, `executor.rs`, `executor/admission/permit.rs`; `WorkerLoopFactory`, `WorkerLoop`, `RuntimeExecutionModel`, `SharedInvocationPermit`, `WatchdogTimer`, `RuntimeMetricsSnapshot` | This plan owns cross-boundary routing. `docs/architecture/runtime/engine-seam.md` owns the stable scheduler/backend seam. `docs/plans/layered-admission-control-plan.md` owns new admission gates only after EIB6 names a protected resource. | Do not add or split a runtime gate until EIB6 records the resource, overload behavior, and metric used to prove it. |
| Runtime backend invocation envelope | `crates/nimbus-runtime/src/backends/mod.rs`, `backends/v8/`, `runtime.rs`, `limits.rs`; `RuntimeBackendFactory`, `RuntimeBackendInvocation`, `RuntimeBackend`, `RuntimeBackendKind`, `RuntimeBundleContentKind`, `RuntimeCompatibilityTarget`, `RuntimePolicy` | This plan owns backend promotion rules. Backend-specific work stays in its owning plan: V8 in the archived seam baseline, Bun/JSC in EIB3 until promoted, wasmtime in `docs/plans/wasmtime-backend-plan.md`. | New backend code must satisfy `docs/architecture/runtime/new-engine-proof-harness.md` before any selectable server/codegen lane exists. |
| Runtime bundle identity and engine cache keys | `crates/nimbus-runtime/src/runtime/bundle.rs`; `RuntimeBundle`, `RuntimeBundleIdentity`, `RuntimeBundleEngineCacheKey`, `verify_integrity()` | `docs/architecture/runtime/engine-seam.md` owns the artifact rules; this plan owns when new content kinds or evaluation formats become selectable. | Do not overload `javascript` or Node target metadata for Bun/JSC or wasmtime. Add explicit artifact fields first. |
| Host-call ABI and runtime capability transport | `crates/nimbus-runtime/src/host.rs`, `runtime/bootstrap/source.rs`, `runtime_capabilities.rs`, `crates/nimbus-server/src/adapters/convex/host_bridge/contract.rs`, `host_bridge/bridge.rs`; `HostBridge`, `HostCallOperation`, `HostCallPayload`, `RuntimeExtensionCall`, `RuntimeGrants`, `RuntimePathPolicy`, `RuntimeEnvPolicy`, `RuntimeCapabilityHost`, `ConvexHostBridge` | EIB2 owns shared trust tiers and capability policy. The archived runtime seam plan owns the Deno/V8 transport baseline. Adapter docs own adapter-specific rejection or dispatch semantics. | Engine transports may differ, but they must preserve ABI version rejection, payload mismatch rejection, sync/async behavior, cancellation, permit pause/resume, metrics, and the provider-neutral extension lane. |
| Generated runtime metadata and server lane selection | `packages/codegen/src/runtime_metadata.mjs`, `packages/codegen/src/main.mjs`, `crates/nimbus-server/src/adapters/convex/manifest.rs`, `registry/resolution/runtime_access.rs`; `runtime_engine`, `runtime_bundle_content_kind`, `runtime_compatibility_target`, `runtime_package_resolution`, `node_runtime_target`, `ConvexRuntimeSelection`, `runtime_lane_for_function()` | This plan owns the cross-boundary rule. EIB2 defines valid policy combinations. EIB3/EIB5 decide whether Bun/JSC or wasmtime are allowed to consume the metadata. | Codegen/server may carry explicit fields, but selectable routing remains V8-only until the relevant backend gate rejects unsupported combinations before invocation. |
| Sandbox lifecycle and catalog seam | `crates/nimbus-sandbox/src/lib.rs`, `backend.rs`, `instance.rs`, `spec.rs`, `crates/nimbus-server/src/sandbox.rs`; `SandboxBackend`, `SandboxBackendKind`, `SandboxSpec`, `SandboxHandle`, `SandboxCatalog`, `SandboxServiceCatalog`, `SandboxServiceLaunch` | EIB4 owns security-audit routing. Future sandbox implementation work should promote a dedicated implementation phase or use `docs/plans/distribution-plan.md` for packaging and shipped artifacts. | Do not hide service sandbox hardening inside runtime-engine work. Sandbox backends have separate lifecycle, endpoint, and packaging owners. |
| krun OCI and microVM hardening | `crates/nimbus-sandbox/src/backends/krun/bundle.rs`, `backends/krun/vm/*`, `patches/crun/*`, `docs/plans/security/sandbox-isolation-audit.md`; OCI `process`, `linux`, `krun.port_map`, `SandboxPortBinding::host_address`, `SandboxResourceLimits` | EIB4 routes each audit finding to implementation, distribution, operator controls, or accepted residual risk. The security audit remains the evidence source until then. | Findings such as seccomp, capabilities, `noNewPrivileges`, TSI bind-address, root VMM lifetime, image provenance, and patched-crun parsing need explicit owners before production microVM exposure. |
| Wasmtime backend | `docs/plans/wasmtime-backend-plan.md`; future `RuntimeBackendKind::Wasmtime`, WIT host imports, fuel or epoch interruption, component/module cache, Store resource limits | Deferred wasmtime plan owns implementation only after activation. EIB5 aligns it to the shared EIB trust/capability vocabulary first. | Wasmtime is a different guest ABI, not a JavaScript target. It must not reuse Node or JavaScript bundle metadata. |
| WASI agent capabilities | `docs/plans/wasi-agent-capabilities-plan.md`; future `nimbus:agent` WIT package, `AgentOsProvider`, component worlds, filesystem/process/http-client capabilities | Deferred WASI agent plan owns implementation only after wasmtime WIT/linker surfaces are stable. EIB5 owns vocabulary alignment. | Agent OS primitives are additive capabilities. Standard runtime functions must not inherit filesystem, process, or outbound HTTP authority by engine selection. |
| Execution resource and admission boundaries | `docs/plans/layered-admission-control-plan.md`, `crates/nimbus-runtime/src/metrics.rs`, `crates/nimbus-engine/src/service/mutations/journal.rs`, `crates/nimbus-storage/src/async_storage/traits.rs`; runtime metrics, mutation CoDel/journal admission, storage executor semaphores | EIB6 owns the resource boundary report. The deferred admission plan owns implementation after a concrete measured slice is promoted. | Every new gate needs a named resource, wait/reject/shed/bypass behavior, and an observable metric before code changes. |
| Engine/storage host paths | `crates/nimbus-server/src/adapters/convex/host_bridge/*`, `crates/nimbus-engine/src/service/mutations/journal.rs`, `service/queries.rs`, `service/scheduler.rs`, `crates/nimbus-storage/src/async_storage/traits.rs`; `ConvexHostBridge`, `MutationExecutionUnit`, `TenantWriteOutcome`, `TenantWriteStorage` | Engine and storage architecture remain the owners of data correctness. This plan only owns execution-boundary routing so new runtimes or sandboxes do not bypass the engine path. | Runtime or sandbox work must route database, scheduler, nested runtime, and service calls through the existing server-to-engine contracts. No alternate mutation/storage path. |

### Overlaps And Gaps

| Issue | Current state | Owner and next action |
| --- | --- | --- |
| Capability vocabulary is split across runtime grants, sandbox specs, future WIT imports, and service endpoint policy. | `RuntimeGrants` is Deno/V8-shaped, sandbox policy is lifecycle/endpoint-shaped, and WASI agent capability design is deferred. | EIB2 defines trust tiers and a shared capability table before any backend promotion. |
| Resource vocabulary is split across runtime limits, sandbox resource limits, wasmtime Store limits, storage executors, and admission plans. | Each layer has a useful local limit, but there is no one resource map for overload decisions. | EIB6 names protected resources and overload semantics before `docs/plans/layered-admission-control-plan.md` can add gates. |
| Bun/JSC has proof evidence but no production owner. | Local Bun proof gates reached timeout/cancel recovery, but permission containment, memory policy, package loading, VM reuse, artifact metadata, and fork posture are not production-ready. | EIB3 records the next Bun/JSC proof gate and the fork/upstream/hold decision. No Nimbus-maintained Bun fork yet. |
| krun sandbox findings need implementation routing. | `docs/plans/security/sandbox-isolation-audit.md` identifies concrete issues, including seccomp, capabilities, `noNewPrivileges`, TSI bind address, root VMM lifetime, image provenance, and crun annotation parsing. | EIB4 routes each finding to implementation, distribution, operator control, or accepted risk. |
| Generated artifact metadata is explicit but currently V8-only. | Codegen emits `runtime_engine`, `runtime_bundle_content_kind`, `runtime_compatibility_target`, and `runtime_package_resolution`; server validation only accepts V8 JavaScript lanes today. | EIB2/EIB3/EIB5 define new legal combinations before routing can select another engine. |
| Active plan list stays small after archiving the runtime seam plan. | `docs/plans/archive/runtime-engine-seam-plan.md` is the historical baseline; wasmtime, WASI agent, and admission plans remain deferred; this plan is the active routing point. | No additional archive needed in EIB1. Retitle or archive only if EIB5/EIB6 exposes overlap after vocabulary alignment. |

## EIB2 Trust Tier Policy

Status: `done` as of 2026-05-21.

`docs/architecture/runtime/permission-model.md` is now the source of truth for
the shared policy vocabulary. It defines these execution trust tiers:

- `in_process_untrusted`
- `in_process_trusted_only`
- `wasm_capability_sandbox`
- `microvm_service`

Settled EIB2 decisions:

- engine names do not imply permissions
- compatibility targets do not imply permissions
- runtime presets lower to mode plus grants, but do not decide trust by
  themselves
- Deno/V8 application JavaScript lanes may run as `in_process_untrusted` only
  when their grants remain within the accepted untrusted subset
- Deno/V8 tooling or operator workloads with `run`, `tool`, `identity`, or
  `Privileged` grants are `in_process_trusted_only`
- Bun/JSC remains proof-only as `in_process_trusted_only` until permission,
  memory, package-loading, lifecycle, artifact-metadata, and fork-posture gates
  pass
- wasmtime components remain deferred under `wasm_capability_sandbox`
- WASI agent filesystem/process/HTTP capabilities are additive imported
  capabilities, not inherited by ordinary WASM functions
- krun/container-backed services are `microvm_service` or the local-dev
  container equivalent; runtime code reaches them through declared service
  bindings rather than direct host authority

## Phase Status Ledger

| Phase | Status | Goal | Verification |
| --- | --- | --- | --- |
| EIB0 | `done` | Create this successor control plane and archive the completed runtime engine seam plan. | Documentation diff check. |
| EIB1 | `done` | Build the execution-boundary ownership map across runtime, sandbox, wasmtime, WASI agent, admission, and security-audit plans. | Source/doc review checklist recorded; `git diff --check`. |
| EIB2 | `done` | Define trust tiers and capability policy shared by in-process engines, WASM components, and sandboxed services. | `docs/architecture/runtime/permission-model.md` updated; no code changes; `git diff --check`. |
| EIB3 | `todo` | Decide Bun/JSC next proof gates and fork posture from permission, memory, package, and lifecycle evidence. | Bun/Nimbus proof transcript or explicit blocked decision. |
| EIB4 | `todo` | Route sandbox isolation audit findings into implementation, distribution, or accepted-risk owners. | Updated security audit and owning plan links; `git diff --check`. |
| EIB5 | `todo` | Align wasmtime and WASI agent plans with the shared trust/capability vocabulary. | Plan updates plus any focused schema/policy tests if code changes. |
| EIB6 | `todo` | Define admission/resource gates only where measured execution or sandbox resources need protection. | Experiment or review report with resource, overload behavior, and metrics. |
| EIB7 | `todo` | Close the unified plan with a go/no-go matrix for runtime backend promotion and sandbox hardening. | All prior phases done and final execution log recorded. |

## Phase Details

### EIB0: Successor Plan And Archive

Status: `done`

Deliverables:

- add this active plan
- archive `docs/plans/runtime-engine-seam-plan.md`
- update active-plan routing in `docs/plans/README.md`
- update runtime seam references so future work starts here

Acceptance criteria:

- the completed runtime plan is no longer listed as active
- future runtime-engine, Bun/JSC, wasmtime, WASI agent, sandbox-hardening, and
  admission-boundary work has one active routing point
- no production code changes

### EIB1: Execution Boundary Ownership Map

Status: `done`

Deliverables:

- inventory execution boundaries in `nimbus-runtime`, `nimbus-sandbox`,
  `nimbus-server`, storage/engine host paths, and generated artifacts
- identify which plan owns each boundary
- identify overlaps, gaps, and plans that should be archived or retitled

Acceptance criteria:

- the map names concrete files, symbols, docs, and active/deferred plans
- no implementation starts from an unowned boundary

Completion notes:

- ownership map recorded above
- source review checklist recorded above
- next phase is EIB2 trust tiers and shared capability policy

### EIB2: Trust Tiers And Capability Policy

Status: `done`

Deliverables:

- define shared trust tiers such as `in_process_untrusted`,
  `in_process_trusted_only`, `wasm_capability_sandbox`, and `microvm_service`
- map runtime grants, sandbox endpoint policy, WASI imports, and agent OS
  providers to those tiers
- state which tiers may access filesystem, network, process, secrets, services,
  identity, and runtime extension calls

Acceptance criteria:

- engine names do not imply permissions
- compatibility targets do not imply permissions
- unsupported capability/tier combinations are rejected or clearly deferred

Completion notes:

- `docs/architecture/runtime/permission-model.md` now defines the shared trust
  tiers and capability matrix
- unsupported future combinations are clearly deferred to EIB3, EIB5, or EIB6
  before implementation
- no code changes, so no focused policy tests were required

### EIB3: Bun/JSC Viability And Fork Decision

Status: `todo`

Deliverables:

- pick the next Bun/JSC proof gate from permission containment, memory limits,
  package/module loading, or VM lifecycle/reuse
- record whether the current local Bun patch shape should be upstreamed,
  discarded, or held as proof-only
- decide what evidence would justify a Nimbus-maintained Bun fork

Acceptance criteria:

- Bun/JSC remains proof-only unless all required evidence exists
- any fork recommendation lists maintenance cost, upstream delta, and exact
  APIs/hooks Nimbus needs

### EIB4: Sandbox Isolation Audit Routing

Status: `todo`

Deliverables:

- route findings from `docs/plans/security/sandbox-isolation-audit.md` to
  implementation owners
- distinguish crun/libkrun patch work, OCI bundle hardening, host firewall
  mitigation, distribution packaging, and operator documentation
- decide which findings block production microVM service exposure

Acceptance criteria:

- every audit finding has an owner, status, and verification path
- sandbox-hardening work does not get hidden inside runtime-engine work

### EIB5: Wasmtime And WASI Agent Alignment

Status: `todo`

Deliverables:

- update wasmtime and WASI agent plans to consume the EIB trust tiers
- align WIT host imports, resource limits, and Store lifecycle with the same
  capability vocabulary used by runtime grants and sandbox policy
- keep wasmtime additive and separate from Convex JavaScript compatibility

Acceptance criteria:

- wasmtime remains deferred until its activation gate is met
- WASI agent capabilities remain deferred until wasmtime host interfaces are
  stable
- plan dependencies are explicit and non-circular

### EIB6: Admission And Resource Boundary Map

Status: `todo`

Deliverables:

- identify the resources each execution class can saturate
- define whether overload waits, rejects, sheds, or bypasses
- connect metrics needed to promote each gate

Acceptance criteria:

- no new admission gate is added without a named protected resource
- overload behavior is explicit before code changes

### EIB7: Promotion Matrix And Closeout

Status: `todo`

Deliverables:

- final matrix for Deno/V8, Bun/JSC, wasmtime, WASI agent, and microVM service
  execution tiers
- explicit go/no-go state for Bun/JSC
- archival or successor-plan recommendations

Acceptance criteria:

- active-plan list stays small
- unresolved implementation work has a clear next owner

## Verification Matrix

| Phase | Minimum verification before `done` |
| --- | --- |
| EIB0 | `git diff --check` for touched docs. |
| EIB1 | Source/docs checklist recorded; `git diff --check`. |
| EIB2 | Architecture docs plus focused policy tests if code changes; otherwise `git diff --check`. |
| EIB3 | Bun/Nimbus proof commands recorded if proof code changes; otherwise `git diff --check`. |
| EIB4 | Security-audit owner updates and any sandbox tests named by the implementation owner. |
| EIB5 | Plan/doc diff checks; focused wasmtime/WASI tests only if code changes. |
| EIB6 | Experiment/report evidence plus metrics route checks if code changes. |
| EIB7 | All prior phases done; final `git diff --check` for docs. |

## Execution Log

| Date | Phase | Status | Notes | Verification |
| --- | --- | --- | --- | --- |
| 2026-05-21 | EIB0 | `done` | Created this active successor plan after the runtime engine seam plan completed RS0-RS6 and Bun/JSC gates 1-10. Archived the completed runtime seam plan as the historical baseline, made this plan the routing point for future runtime-engine, Bun/JSC, wasmtime, WASI agent, sandbox isolation, and execution-admission work, and added the autonomous `/goal` prompt for this plan. | Documentation-only change; `git diff --check` passed for touched docs. |
| 2026-05-21 | EIB1 | `done` | Recorded the execution-boundary ownership map across runtime scheduling, backend invocation, bundle metadata, host-call transport, generated artifact metadata, server runtime selection, sandbox lifecycle, krun/OCI hardening, wasmtime, WASI agent capabilities, admission/resource gates, and engine/storage host paths. Confirmed the next implementation gate is EIB2 trust tiers and capability policy, not Bun/JSC or sandbox implementation. | Source/doc review checklist recorded; `git diff --check -- docs/plans/execution-isolation-and-runtime-backends-plan.md` passed. |
| 2026-05-21 | EIB2 | `done` | Added the shared execution trust-tier vocabulary and capability matrix to `docs/architecture/runtime/permission-model.md`, then recorded the settled assignments in this plan: Deno/V8 application lanes may be `in_process_untrusted`, privileged/tooling in-process work is `in_process_trusted_only`, Bun/JSC remains proof-only/trusted-only, wasmtime and WASI agent capabilities remain deferred under `wasm_capability_sandbox`, and sandboxed services are `microvm_service`. | Documentation-only change; `git diff --check -- docs/architecture/runtime/permission-model.md docs/plans/execution-isolation-and-runtime-backends-plan.md` passed. |
