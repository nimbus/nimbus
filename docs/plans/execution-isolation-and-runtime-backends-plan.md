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
- **Architecture references:**
  - `docs/architecture/runtime/engine-seam.md`
  - `docs/architecture/runtime/new-engine-proof-harness.md`
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
| `docs/plans/security/sandbox-isolation-audit.md` | Security findings that must be routed into sandbox hardening or accepted-risk decisions. |
| `docs/plans/wasmtime-backend-plan.md` | Deferred backend-specific wasmtime execution plan; consumes this plan's trust and capability vocabulary. |
| `docs/plans/wasi-agent-capabilities-plan.md` | Deferred agent capability plan; consumes this plan's capability and sandbox tiers. |
| `docs/plans/layered-admission-control-plan.md` | Deferred admission-control plan; consumes this plan's resource boundary map before adding gates. |
| `docs/plans/distribution-plan.md` | Owns packaging and distribution once execution/sandbox artifacts need shipping. |

## Phase Status Ledger

| Phase | Status | Goal | Verification |
| --- | --- | --- | --- |
| EIB0 | `done` | Create this successor control plane and archive the completed runtime engine seam plan. | Documentation diff check. |
| EIB1 | `todo` | Build the execution-boundary ownership map across runtime, sandbox, wasmtime, WASI agent, admission, and security-audit plans. | Source/doc review checklist plus `git diff --check`. |
| EIB2 | `todo` | Define trust tiers and capability policy shared by in-process engines, WASM components, and sandboxed services. | Architecture doc update plus focused policy tests if code changes. |
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

Status: `todo`

Deliverables:

- inventory execution boundaries in `nimbus-runtime`, `nimbus-sandbox`,
  `nimbus-server`, storage/engine host paths, and generated artifacts
- identify which plan owns each boundary
- identify overlaps, gaps, and plans that should be archived or retitled

Acceptance criteria:

- the map names concrete files, symbols, docs, and active/deferred plans
- no implementation starts from an unowned boundary

### EIB2: Trust Tiers And Capability Policy

Status: `todo`

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
| 2026-05-21 | EIB0 | `done` | Created this active successor plan after the runtime engine seam plan completed RS0-RS6 and Bun/JSC gates 1-10. Archived the completed runtime seam plan as the historical baseline and made this plan the routing point for future runtime-engine, Bun/JSC, wasmtime, WASI agent, sandbox isolation, and execution-admission work. | Documentation-only change; `git diff --check` passed for touched docs. |
