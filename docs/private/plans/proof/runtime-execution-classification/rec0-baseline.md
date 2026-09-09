# REC0 Baseline Audit And Verifier Scaffold

Status: `done`.

REC0 establishes the current-state inventory and proof surface for
`docs/private/plans/runtime-execution-classification-plan.md`. It does not
change runtime behavior. REC1 may now introduce typed internal classifier
outputs because this proof records the existing scheduler, host-call, profile,
tenant-budget, host-governance, affinity, pool-reuse, codegen, and host-session
seams.

PIR baseline command:

```text
bash scripts/verify-profile-aware-isolate-runtime.sh
```

PIR baseline result recorded before REC0 closeout:

```text
Summary: 90 passed, 0 failed
```

## Current-State Diagram

```text
adapter/codegen evidence
  -> InvocationRequest { InvocationKind, auth, services, bundle identity }
  -> RuntimeWorkerJob
  -> RuntimeExecutorAdmission
     - tenant queue/in-flight budget
     - RuntimeHostResourceDecision / RuntimeHostWorkClass
  -> RuntimeWorkerRouter
     - RuntimeAffinityKey for locality
  -> cooperative worker loop
     - direct InvocationKind::allows_cooperative_multiplexing checks
  -> V8 runtime / warm pool
     - RuntimePoolPartitionKey for exact retained-runtime reuse
  -> HostCallEnvelope / HostCallPayload / HostCallOperation
     - Convex host bridge dispatch validates host_call_session_id first
```

REC1 must insert `RuntimeExecutionPlan` between admitted request facts and
scheduler/pool decisions. `InvocationKind` remains a Convex semantic label.

## Inventory

| Symbol or seam | Current paths | Current role | REC owner decision | Complexity pocket |
|---|---|---|---|---|
| `InvocationKind` | `crates/nimbus-runtime/src/runtime/invocation.rs`; server, Convex, Cloud Functions, tests construct requests | Semantic function kind | Keep as semantic kind only | Scheduler predicate spread |
| `InvocationKind::allows_cooperative_multiplexing` | `crates/nimbus-runtime/src/runtime/invocation.rs`; consumed by `worker_loop/cooperative/run.rs` and `worker_loop/cooperative/execution.rs` | Direct scheduler eligibility predicate | REC3 removes scheduler ownership from `InvocationKind` | Scheduler predicate spread |
| `RuntimeWorkerJob` | `crates/nimbus-runtime/src/executor/queue/job.rs`; constructed in `executor/invoke.rs`; routed/admitted by queue/admission modules | Carrier for host, bundle, request, context, cancellation, response-ready sender, dispatch handle | REC3 adds or carries `RuntimeExecutionPlan` here or in its admission context | Job/admission coupling |
| `RuntimeProfile` | `crates/nimbus-runtime/src/limits/profile.rs`; re-exported through runtime facade; consumed by PIR/NFR snapshot and metrics paths | Runtime surface/startup evidence | Keep efficiency/substrate only | Tenant-admission versus runtime efficiency |
| `RuntimeEfficiencyPlan` | `crates/nimbus-tenant/src/runtime_profile.rs`; tests in `crates/nimbus-tenant/src/tests.rs` | Tenant-owned profile/effective pool evidence | REC1 narrows to admission/profile evidence or retires after runtime-owned plan exists | Tenant-admission versus runtime efficiency |
| `RuntimeTenantBudget` | `crates/nimbus-runtime/src/limits/resources.rs`; lowered through `nimbus-tenant/src/policy_input.rs`; surfaced in `nimbus-server/src/protocol.rs` | Tenant quota/resource budget evidence | REC consumes as budget input, not scheduler replacement | Job/admission coupling |
| `RuntimeHostWorkClass` | `crates/nimbus-runtime/src/limits/pressure.rs`; selected today by `runtime_host_work_class_for_job` in `executor/admission.rs` | PIR7 host-pressure work class | REC3 consumes/extends this vocabulary; no sibling host class | Job/admission coupling |
| `RuntimeHostResourceDecision` | `crates/nimbus-runtime/src/limits/pressure.rs`; `limits/policy.rs`; `executor/admission.rs`; metrics | Host resource admission decision | REC admission outcome wraps or consumes this decision | Verifier drift |
| `RuntimeAffinityKey` | `crates/nimbus-runtime/src/affinity.rs`; `executor/queue/router.rs`; `backends/v8/warm_pool.rs` | Locality key for router and current warm pool partition input | Do not reuse as `RuntimePoolAuthorityKey`; authority key must be exact and broader | Routing locality versus authority reuse |
| `RuntimePoolPartitionKey` | `crates/nimbus-runtime/src/backends/v8/warm_pool.rs` | Exact retained-runtime reuse key today | REC1/REC3 align with future `RuntimePoolAuthorityKey` | Routing locality versus authority reuse |
| Pool reuse | `backends/v8/warm_pool.rs`; `runtime/driver/invocation.rs`; `worker_loop/cooperative/retention.rs`; metrics | Warm runtime hit/miss/retire/retain behavior | REC consumes plan and authority key before reuse | Routing locality versus authority reuse |
| `HostCallOperation` | `crates/nimbus-runtime/src/host.rs` | Typed host operation enum | REC2 classifies exhaustively near this enum | Host-call ABI/adapter dispatch |
| `HostCallPayload::operation` | `crates/nimbus-runtime/src/host.rs` | Typed payload-to-operation mapping | REC2 uses this as effect-observation source | Host-call ABI/adapter dispatch |
| `HostCallEnvelope` | `crates/nimbus-runtime/src/host.rs` | ABI envelope and operation carrier | REC2 observes effects after envelope parse | Host-call ABI/adapter dispatch |
| Convex host dispatch | `crates/nimbus-server/src/adapters/convex/host_bridge/async_bridge/dispatch.rs` | Groups payloads by function, query builder, query read, document, scheduler, service/nested, extension dispatch | REC2 keeps adapter dispatch separate from typed effect classification | Host-call ABI/adapter dispatch |
| Host-call session validation | `crates/nimbus-bridge/src/state.rs`; `crates/nimbus-bridge/src/capabilities.rs`; Convex bridge validation before dispatch | Active session/provenance guard | REC2/REC3 make this explicit proof for cooperative interleaving | Async provenance |
| Codegen context evidence | `packages/codegen/src/planner/context_api.mjs`; `packages/codegen/src/emit/runtime_bundle_query_helpers.mjs`; `runtime_bundle_mutation_helpers.mjs`; `runtime_bundle_action_helpers.mjs` | Static context and operation hints | REC4 may consume as evidence only; host-op observation remains enforcement | Context narrowing versus JS ambient authority |

## Direct Scheduler Consumers

Current production direct consumers of
`InvocationKind::allows_cooperative_multiplexing`:

- Definition: `crates/nimbus-runtime/src/runtime/invocation.rs`.
- Worker admission deferral:
  `crates/nimbus-runtime/src/worker_loop/cooperative/run.rs`.
- Slot start path:
  `crates/nimbus-runtime/src/worker_loop/cooperative/execution.rs`.

REC3 must remove direct scheduler decisions from `InvocationKind`. Until then,
the current behavior remains the compatibility baseline: `Query` and
`PaginatedQuery` are eligible, `Mutation` and `Action` run directly.

## Host Operation Inventory

`HostCallOperation` currently contains these typed operations:

- `HttpRoute`
- `CtxQuery`
- `CtxPaginatedQuery`
- `CtxMutation`
- `CtxAction`
- `CtxRunQuery`
- `CtxRunMutation`
- `CtxRunAction`
- `DocumentGet`
- `QueryBuilderStart`
- `QueryBuilderWithIndex`
- `QueryBuilderFilter`
- `QueryBuilderOrder`
- `QueryReadCollect`
- `QueryReadTake`
- `QueryReadPaginate`
- `QueryReadFirst`
- `QueryReadUnique`
- `DocumentInsert`
- `DocumentPatch`
- `DocumentDelete`
- `CtxSchedulerRunAfter`
- `CtxSchedulerRunAt`
- `CtxSchedulerCancel`
- `CtxServiceLookup`
- `CtxRuntimeEnterNestedCall`
- `RuntimeExtensionCall`

REC2 must classify each operation into an explicit effect class. The initial
taxonomy is: pure/local read, observable read, write, scheduler, service or
external, nested runtime, extension, HTTP route, and unknown. Unknown or
unclassified operations are cooperative-ineligible.

## Canonical Pattern Carry-Forward

REC0 carries forward the plan's exemplar conclusion: modern runtimes and
schedulers derive an execution plan from independent facts. Workerd, Convex,
Deno/deno_core, Supabase Edge Runtime, OpenWorkers, Wasmtime, Kubernetes, and
Nomad all separate semantic kind, runtime surface, capability/effect, resource
budget, pool authority, scheduler admission, and enforcement. REC rejects a
monolithic `RuntimeExecutionClass` that directly implies authority, placement,
pooling, and admission.

Deletion test: if the future REC module is deleted and the same matches return
to `InvocationKind`, the worker loop, host bridge, and tenant policy, the module
is too shallow. The module must make the worker loop consume a compact plan and
let tests cover the classifier without constructing a full runtime.

## Validation Answers

- OVQ-01 side-channel posture inputs: unknown posture is ineligible. Required
  inputs include timers, shared memory, native/FFI/process, Node built-ins,
  inspector/debug, external service grants, runtime profile, and isolation tier.
- OVQ-02 pool authority key completeness: missing tenant/principal, runtime
  profile, permission profile, exact service grants, bundle/provenance,
  env/secrets version, generated context shape, module graph/cache, Node
  surface, or host session facts force fresh context or fresh isolate.
- OVQ-03 read-only versus observable: pure/local reads are distinct from
  observable reads. Auth identity, env/secrets, storage metadata, scheduler
  metadata, time/random, and external reads are not automatically cooperative
  safe.
- OVQ-04 nested runtime matrix: start with Convex-compatible nested-call rules
  as evidence, but classify nested runtime calls explicitly. Unknown or broader
  nesting is ineligible.
- OVQ-05 NodeFull cooperative eligibility: NodeFull starts ineligible for
  cooperative reuse unless side-channel posture, host effects, loader behavior,
  and realm semantics are proven.
- OVQ-06 codegen/context narrowing: codegen context shape is useful evidence,
  not enforcement. Runtime host-op observation remains authoritative.
- OVQ-07 effect granularity: REC2 may refine operations by parameters only when
  needed; the minimum is exhaustive classification over `HostCallOperation`.
- OVQ-08 numeric thresholds: scheduling class thresholds must come from PIR
  metrics and budgets; no hard-coded semantic-kind CPU assumptions.
- OVQ-09 metadata/runtime conflict: observed write, service, scheduler, nested,
  external, extension, or unknown effects override static metadata and make the
  invocation ineligible or fail with a typed effect violation.
- OVQ-10 isolation-tier escape hatches: native process, FFI, inspector, debug,
  and privileged services require higher isolation tier or ineligibility, not a
  normal read effect.
- OVQ-11 JavaScript global authority: narrowed `ctx` is partial unless globals,
  primordials, dynamic import, module cache, timers, random, and Node/Web APIs
  cannot expose equivalent authority.
- OVQ-12 async context provenance: existing `RuntimeHostState` session checks
  are necessary but not sufficient; REC3 must prove promises/callbacks/streams
  and host-owned resources cannot resume under another session.
- OVQ-13 observable output gates: effectful work remains run-to-completion until
  REC proves existing engine/storage atomicity is enough or adds lifecycle gates.
- OVQ-14 determinism and virtualized reads: time, random, env/secrets, auth
  identity, storage metadata, external reads, and scheduler metadata default to
  observable effects unless virtualized or recorded.
- OVQ-15 overload and criticality source: no per-kind criticality is invented.
  Queueing, shedding, tenant quota, and host pressure become explicit admission
  outcomes using tenant and PIR7 budget facts.
- OVQ-16 future capability-resource alignment: NimbusFS, object storage,
  service bindings, and egress should appear as typed capability handles,
  effect subclasses, pool-authority inputs, or isolation-tier inputs before
  global host operations expand.

REC0 default: Unknown or unclassified operations are cooperative-ineligible.
REC0 default: NodeFull starts ineligible for cooperative reuse.

## Verification

```text
bash -n scripts/verify-runtime-execution-classification.sh
```

Expected result: exits successfully.

```text
bash scripts/verify-runtime-execution-classification.sh
```

Expected verifier closeout: `Summary: 8 passed, 0 failed`.

## REC1 Handoff

REC1 may now introduce internal typed classifier outputs without public API
changes. It must keep `nimbus-runtime` zero-workspace-dep, narrow or retire
`nimbus-tenant::RuntimeEfficiencyPlan` before scheduler policy grows, and keep
PIR7's host-governance vocabulary instead of creating a parallel host-admission
model.
