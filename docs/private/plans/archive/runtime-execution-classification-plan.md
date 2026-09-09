# Plan: Runtime Execution Classification (REC)

Canonical plan for replacing ad hoc runtime scheduling predicates with one
derived internal execution plan. The purpose is to keep correctness facts,
runtime substrate facts, effect/capability facts, and measured behavior separate
before Nimbus decides whether to multiplex, pin, warm-pool, shed, or scale an
invocation.

This plan exists because `InvocationKind` is currently useful as a Convex
semantic label, but it is too small an interface for isolate scheduling. A query
can require Node/npm, consume CPU, request services, or reach effectful host
operations. A mutation can use the Web-lean runtime surface. Pool sizing, worker
pinning, CPU fairness, and host isolation cannot safely be inferred from
`query`/`mutation`/`action` alone.

`docs/private/plans/profile-aware-isolate-runtime-plan.md` (PIR) is the
benchmarking and profiling foundation for this work. REC does not invent a
second measurement methodology. It consumes PIR's profile matrix, current-RSS
methodology, synthetic-await and CPU-bound lanes, host resource budget metrics,
and verifier discipline to prove that the new classifier keeps the same numeric
performance posture while making safety decisions explicit.

---

## Control Plan Rules

Source of truth:

1. the current git worktree
2. this plan's `Phase Status Ledger`, `Implementation Checkpoints`, and
   `Execution Log`
3. `docs/private/plans/profile-aware-isolate-runtime-plan.md` for PIR profiles,
   benchmark harnesses, numeric proof rules, and host resource guardrails
4. `/private/tmp/nimbus-execution-classifiers.html` as the originating design
   sketch until REC0 copies the durable claims into this plan/proof artifacts
5. the runtime/server/tenant code files named by each REC band

Do not rely on prior chat transcripts as progress state.

### Status Model

- `todo`: not started; eligible when hard dependencies and gate notes are
  satisfied
- `in_progress`: actively being implemented; keep exactly one REC band in this
  state per autonomous execution run
- `blocked`: cannot proceed until the recorded blocker is resolved
- `done`: acceptance criteria are met and verification has been recorded
- `deferred`: intentionally parked behind a product or benchmarking gate

### Status

- **Status:** `done`
- **Lane:** L3 - runtime / WASM
- **Primary owner:** this plan
- **Archive state:** archived complete local control plane on 2026-06-27 after
  the REC4 `nestedCalls` runtime/codegen context-shape proof was closed and
  `bash scripts/verify-runtime-execution-classification.sh` passed with
  `Summary: 24 passed, 0 failed`.
- **Foundation:** PIR0/PIR1/PIR4/PIR7 benchmarking and profiling methodology
- **Activation gate:** REC should run before any PIR follow-on that makes
  cooperative multiplexing, warm-pool placement, adaptive defaults, or host
  pressure decisions depend on function kind. Existing PIR4 behavior may remain
  as the compatibility baseline while REC introduces the deeper internal module.
  REC must complete before NFR2-NFR6 can implement NodeFull realm leases, because
  that work needs the canonical execution-plan, authority-key, effect-class, and
  async-provenance vocabulary.
- **Current PIR baseline:** `bash scripts/verify-profile-aware-isolate-runtime.sh`
  passed locally on 2026-06-21 with `90 passed, 0 failed`; PIR0..PIR7 are the
  REC foundation and PIR8 remains deferred.

### Recovery Loop

1. Read this plan, PIR's `Control Plan Rules`, PIR's `Phase Status Ledger`, and
   PIR's benchmark/proof sections.
2. Run or inspect `bash scripts/verify-profile-aware-isolate-runtime.sh` and
   record the exact pass/fail count before starting REC work.
3. Inspect the current git worktree and reconcile it against both plans. PIR is
   actively changing runtime files in this branch; treat those changes as
   progress state, not drift to revert.
4. Resume the single REC band marked `in_progress`, if any.
5. Keep code edits behind internal seams; do not add user-facing efficiency
   knobs.
6. Record exact verification, including benchmark numbers where a band changes
   scheduling, pooling, or host-op hot paths.
7. Update this plan, proof artifacts, and verifier expectations before stopping.

---

## Readiness Audit (2026-06-21)

**Verdict:** ready for REC0 implementation after this audit update. PIR's
control-plane verifier is green through PIR7, and the current tree contains the
exact ad hoc scheduler predicates REC is meant to replace. REC0 is still the
required first implementation band because the current-state inventory and REC
verifier do not exist yet.

Current live facts from the code review:

- `InvocationKind::is_convex_read_semantic_candidate()` lives in
  `crates/nimbus-runtime/src/runtime/invocation.rs` and currently returns true
  for `Query` and `PaginatedQuery` only. REC0 recorded the previous
  `allows_cooperative_multiplexing` name as the pre-REC1 baseline.
- Direct scheduler consumers are in
  `crates/nimbus-runtime/src/worker_loop/cooperative/run.rs` and
  `crates/nimbus-runtime/src/worker_loop/cooperative/execution.rs`.
- `RuntimeWorkerJob` currently carries `host`, `bundle`, `request`, `context`,
  cancellation, response-ready sender, and dispatch handle, but no derived
  execution plan (`crates/nimbus-runtime/src/executor/queue/job.rs`).
- PIR's profile and budget facts now live at
  `crates/nimbus-runtime/src/limits/profile.rs`,
  `crates/nimbus-runtime/src/limits/resources.rs`,
  `crates/nimbus-runtime/src/limits/pressure.rs`, and
  `crates/nimbus-tenant/src/runtime_profile.rs`.
- `RuntimeEfficiencyPlan` is currently tenant-owned and carries profile plus
  effective pool/execution-model evidence. REC must reconcile this so
  `nimbus-tenant` remains the admission/evidence owner while `nimbus-runtime`
  owns scheduler, pool, and cooperative execution decisions.
- `HostCallOperation`, `HostCallPayload`, `HostCallEnvelope`, and
  `HostCallPayload::operation()` live in
  `crates/nimbus-runtime/src/host.rs`; the Convex host bridge dispatches those
  payloads through
  `crates/nimbus-server/src/adapters/convex/host_bridge/async_bridge/dispatch.rs`.
- Host-call session validation is already centralized in
  `crates/nimbus-bridge/src/state.rs` and repeated by the Convex bridge before
  dispatch. REC2 must not weaken that binding.
- Codegen already has kind-specific planning/context hints in
  `packages/codegen/src/planner/context_api.mjs`,
  `packages/codegen/src/emit/runtime_bundle_query_helpers.mjs`,
  `packages/codegen/src/emit/runtime_bundle_mutation_helpers.mjs`, and
  `packages/codegen/src/emit/runtime_bundle_action_helpers.mjs`. Those hints are
  useful REC evidence, not the trust boundary.

Readiness issues found and folded into this plan:

1. REC0's hard dependency must include PIR7's host-resource budget and
   controller proof, not only PIR0/PIR1/PIR4.
2. REC's inventory path must use the current singular
   `crates/nimbus-runtime/src/limits/profile.rs` path.
3. REC1 must explicitly decide whether to retire, rename, or narrow
   `nimbus-tenant::RuntimeEfficiencyPlan`; it cannot leave tenant-owned
   efficiency evidence as the place where future pool/scheduler policy grows.
4. REC2 needs an exhaustive host-operation effect classifier tied to the
   `HostCallOperation` enum, with verifier checks that fail when a new operation
   lacks an effect classification.
5. REC3/REC5 must consume PIR7's `RuntimeHostWorkClass` and
   `RuntimeHostResourceDecision` instead of inventing a parallel host admission
   work-class hierarchy.

## Full Architecture Audit (2026-06-21)

**Verdict:** the plan is cohesive and ready to drive REC0 through REC5 after
this audit update. The implementation shape is maintainable only if REC becomes
a deep internal module whose interface hides the current cross-cutting scheduler
knowledge from callers. REC is not a naming pass over
`InvocationKind::is_convex_read_semantic_candidate()`; it is the seam that keeps
semantic kind, runtime profile, host effects, pool authority, scheduler
admission, host pressure, and tenant budgets from leaking into each other.

Current audit facts that must stay true during implementation:

- `InvocationKind` is a shallow interface for scheduling today: it exposes
  `is_convex_read_semantic_candidate()` and the cooperative worker loop calls it
  directly from `worker_loop/cooperative/run.rs` and
  `worker_loop/cooperative/execution.rs`.
- `RuntimeWorkerJob` is the right carrier seam for a derived plan, but it must
  not become a bag of duplicated facts. The execution plan should be constructed
  once before worker admission and then passed by value or reference through the
  worker loop.
- `RuntimeAffinityKey` is a routing/locality optimization, not a
  `RuntimePoolAuthorityKey`. The future pool authority key must include
  authority-bearing facts such as tenant/principal, exact service grants,
  bundle/provenance hash, runtime profile, permission profile, env/secrets
  version, module graph/cache key, generated context shape, and host-bridge
  session class. Reusing `RuntimeAffinityKey` as the authority key would be a
  security regression.
- `RuntimeHostWorkClass` and `RuntimeHostResourceDecision` already form the PIR7
  host-pressure/admission vocabulary. REC may consume or extend them through the
  execution plan, but it must not create a sibling host-admission enum family.
- `nimbus-tenant::RuntimeEfficiencyPlan` currently mixes admission evidence with
  effective pool/execution-model facts. REC1 must either narrow it into
  tenant-owned admission/profile evidence or remove it after runtime-owned
  execution decisions move into REC.
- `HostCallOperation` is the correct typed axis for effect classification. The
  classifier must be exhaustive over the enum and must use typed error variants
  when observed effects conflict with the admitted plan.
- Host-call session validation already exists in `nimbus-bridge::RuntimeHostState`
  and is repeated before Convex bridge dispatch. REC must make that validation a
  proof obligation for cooperative interleaving, not an assumed side effect.
- Codegen context proxies and runtime-bundle helpers are useful static evidence,
  but JavaScript global authority and runtime host-op observation remain the
  enforcement seam.
- `scripts/verify-runtime-execution-classification.sh` now exists. Later bands
  must grow it before claiming completion.

### Complexity Pockets To Contain

1. **Scheduler predicate spread.** Direct calls to
   `InvocationKind::is_convex_read_semantic_candidate()` are easy to understand
   locally but shallow globally. REC3 must remove every scheduler/pool/autoscale
   decision from `InvocationKind` and route those decisions through
   `RuntimeExecutionPlan`.
2. **Job construction and admission coupling.** The job queue, tenant fairness,
   host pressure, worker routing, cooperative scheduler, and runtime start path
   are close together. Keep classification pure and construct the execution plan
   before admission; admission should consume the plan instead of recomputing
   semantic/profile/effect facts.
3. **Host-call ABI and adapter dispatch.** `HostCallPayload` parsing,
   `HostCallOperation`, Convex dispatch, and shared runtime capabilities can
   drift if effect classification is implemented as string matching. Keep
   effect classification typed and colocated with the operation enum, while
   adapter-specific semantics stay under adapter ownership.
4. **Routing locality versus authority reuse.** Worker affinity and warm-pool
   authority look similar but answer different questions. Routing can be
   approximate and eventually consistent; authority reuse must be exact and
   fail-closed.
5. **Tenant admission versus runtime efficiency.** Tenant isolation/admission
   owns whether work may run and under which grants. Runtime owns how an already
   admitted invocation is scheduled, pooled, and observed. Do not let
   `RuntimeEfficiencyPlan` become a second runtime scheduler hidden in
   `nimbus-tenant`.
6. **Context narrowing versus JavaScript ambient authority.** Reader-only
   `ctx` shapes are not enough if equivalent authority remains on globals,
   mutable primordials, module cache/import paths, timers, randomness, Node
   built-ins, or package-provided APIs. REC4 must record whether context
   narrowing is complete or only supporting evidence.
7. **Telemetry cardinality.** Effect class, eligibility reason, host work class,
   admission action, pressure level, and profile are acceptable low-cardinality
   labels. Tenant IDs, function names, bundle hashes, and session IDs belong in
   bounded debug/proof artifacts, not always-on metrics.
8. **Verifier drift.** The plan is long and cross-cutting. Each band must update
   the ledger, proof artifact, verifier expectations, and execution log together;
   a band marked `done` without its proof file and exact command output is
   invalid.

### Rust Idiom And Resilience Contract

- Model classifier outputs as closed enums with `#[serde(rename_all =
  "snake_case")]` where they are serialized for diagnostics or proof artifacts.
  Keep operator/user input parsing explicit and fail-closed.
- Use typed newtypes for authority and budget quantities that must not be
  confused. Prefer names such as `RuntimePoolAuthorityKey`, `RuntimeEffectClass`,
  `CooperativeIneligibilityReason`, `RuntimeAdmissionOutcome`,
  `RuntimeSchedulingClass`, `CpuMillis`, and `RuntimeSeatCount` over raw
  `String`, `usize`, or stringly reason codes.
- Use `NonZeroUsize`/`NonZeroU32` for non-zero limits and `Duration` for time.
  Keep raw milliseconds and CPU millicores at CLI, serialization, and proof-file
  edges only.
- Keep classification pure, deterministic, and cheap. Host pressure readers,
  metrics, clocks, and runtime observations are adapters or inputs to the pure
  classifier, not global state accessed inside it.
- Prefer `#[must_use]` on plan/decision/result types where ignoring them would
  silently bypass classification, admission, or effect enforcement.
- Use typed error variants for classification conflicts, unclassified host
  operations, effect violations, stale host-call sessions, queue full, host
  pressure shed, and missing safety posture. Avoid string-only contract errors at
  the new seams.
- Keep `nimbus-runtime` zero-workspace-dep. If REC needs tenant/admission facts,
  pass them in as runtime-local data shapes; do not import workspace policy
  crates into `nimbus-runtime`.
- Keep `nimbus-core` zero-I/O. Do not move runtime classification or effect
  observation there merely to share types.
- Use small ownership-based modules, not pass-through helpers. Acceptable module
  names include `execution_plan`, `effects`, `eligibility`, `authority_key`,
  `scheduling_class`, `admission_outcome`, and `observed_effects`; avoid
  `helpers`, `misc`, and caller-shaped wrappers.
- Apply the deletion test: if deleting a module simply moves the same match
  statements into the worker loop, host bridge, and tenant policy, the module is
  too shallow. The REC module earns its keep only when the worker loop can ask a
  small interface what to do and tests can cover the classifier without spinning
  a runtime.

## Architecture Shape

REC introduces one internal deep module:

```rust
RuntimeExecutionPlan {
    function_kind: InvocationKind,
    runtime_profile: Option<RuntimeProfile>,
    effect_class: RuntimeEffectClass,
    cooperative_eligibility: CooperativeEligibility,
    pool_authority_key: RuntimePoolAuthorityKey,
    scheduling_class: RuntimeSchedulingClass,
    tenant_budget: RuntimeTenantBudget,
}
```

The exact Rust shape may differ, and admission outcomes may live beside rather
than inside the plan, but the separation must not:

- **Semantic kind** answers: what Convex function contract is this?
- **Runtime profile** answers: which JS runtime surface and startup shape are
  required?
- **Effect class** answers: what host operations and capabilities can this
  invocation touch, and whether read effects are pure/local or observable?
- **Cooperative eligibility** answers: may this invocation interleave at async
  yield points?
- **Pool authority key** answers: which tenant/profile/grant/bundle facts permit
  runtime reuse?
- **Scheduling class** answers: is the work CPU-heavy, I/O-heavy, memory-heavy,
  or unknown based on measured behavior?
- **Budgets** answer: what tenant/operator/host limits apply?
- **Admission outcome** answers: did tenant quota, host pressure, classifier
  ineligibility, queue capacity, or cancellation admit, queue, shed, or reject
  the invocation?

### Canonical Exemplar Audit (2026-06-21)

Verdict: this is a solved architecture family, but not as a single magic
"execution class" enum. The modern pattern is a derived execution plan assembled
from independent facts: semantic function kind, runtime surface, host
capabilities/effects, resource budgets, pool authority, scheduler admission, and
runtime enforcement. REC should copy that shape and avoid inventing
Nimbus-specific policy names where a canonical term is clearer.

Primary local exemplars reviewed under `/Users/jack/src/github.com/*` plus
GitHub repository search:

| Exemplar | Relevant source | Pattern REC should copy |
|---|---|---|
| Cloudflare `workerd` (`cloudflare/workerd`) | `src/workerd/io/io-context.*`, `limit-enforcer.h`, `worker.c++` | Per-request `IoContext`, capability-bearing subrequest channels, `LimitEnforcer`, event-specific lifecycle (`waitUntil`, scheduled, actor), and isolate async-lock scheduling are separate responsibilities. Function/event kind does not own all scheduling and authority decisions. |
| Convex backend (`get-convex/convex-backend`) | `crates/common/src/types/functions.rs`, `crates/isolate/src/environment/*`, `udf/async_syscall.rs` | `UdfType` is semantic. Runtime environment/syscall traits and async syscall batches carry capability/effect facts. Nested UDF calls are allowed by an explicit matrix. Query labels alone are not enough authority. |
| Deno / `deno_core` (`denoland/deno`, `denoland/deno_core`) | `runtime/worker.rs`, `core/io/resource_table.rs`, `ops/op2/README.md` | Extensions, `OpState`, permissions, and `ResourceTable` are the host boundary. Async op eager/lazy/deferred knobs are performance mechanics, not semantic authority or tenant policy. |
| Supabase Edge Runtime (`supabase/edge-runtime`) | `crates/base/src/worker/pool.rs`, `worker/driver/managed.rs`, `crates/cpu_timer/src/lib.rs` | Worker pool policy, max parallelism, request wait timeout, CPU accounting, memory accounting, and supervisor termination are independent from request dispatch. |
| OpenWorkers (`openworkers/*`) | `openworkers-runner/src/task_executor.rs`, `worker_pool.rs`, `openworkers-runtime-v8/src/pool*.rs` | Thread-local V8 pools, tenant/owner keyed isolates, warm context reuse, fail-fast pool acquisition, queue backpressure, and per-owner limits are explicit pool/admission facts. |
| Wasmtime (`bytecodealliance/wasmtime`) | `runtime/limits.rs`, `runtime/store.rs`, `engine.rs` | Resource limiters, fuel, epoch interruption, and call hooks are runtime enforcement mechanisms. They should consume a policy/budget, not replace classification. |
| `isolated-vm` / `isolator` | `isolated-vm/src/isolate/*`, `isolator/src/{manager,runtime}.rs` | V8 scheduling, cross-isolate task phases, execution timeout, and CPU termination are low-level guards. Useful for enforcement posture, not for Convex semantic classification. |
| Kubernetes / Nomad | Kubernetes `PodSpec`/`ResourceRequirements`/`RuntimeClassName`; Nomad scheduler worker config validation | Mature schedulers keep workload shape, resource requests, runtime class, security context, constraints, and scheduler worker configuration as separate validated dimensions. |

Additional GitHub candidates checked but not cloned because they did not change
the REC architecture conclusion: `fastly/js-compute-runtime`,
`awslabs/llrt`, and `bytecodealliance/javy`. They are useful references for
runtime packaging, startup, and JavaScript/Wasm embedding, but the stronger
patterns for Nimbus's host-call classification and isolate-pool scheduler are
already covered by Workerd, Convex, Deno, Supabase Edge Runtime, OpenWorkers,
Wasmtime, and the scheduler systems above.

Research inputs REC0 must carry forward:

- **Capability security / least authority:** Cap'n Proto RPC treats interface
  references as capabilities that both designate an object and confer permission
  to call it. REC should prefer typed capability/context surfaces over singleton
  global host access when narrowing query/mutation/action runtime contexts.
  Source: https://capnproto.org/rpc.html
- **Sandbox fail-closed permission posture:** Deno is no-I/O-by-default, scopes
  grants, warns that `run`/FFI-style permissions bypass the sandbox, and
  terminates on malformed permission-broker state to preserve integrity. REC
  should preserve fail-closed host-operation classification and treat native or
  subprocess surfaces as isolation-tier concerns, not normal read effects.
  Source: https://docs.deno.com/runtime/fundamentals/security/
- **Filter then score scheduler shape:** Kubernetes separates feasibility
  filtering from scoring over resource, policy, affinity, locality, and
  interference dimensions. REC should treat cooperative eligibility as a filter
  and scheduling class as scoring/placement evidence, not a single blended enum.
  Source:
  https://kubernetes.io/docs/concepts/scheduling-eviction/kube-scheduler/
- **Workload heterogeneity:** Borg and production serverless workload studies
  show large variation in CPU, memory, latency sensitivity, cold starts, and
  burstiness. REC should keep warm-pool sizing and multiplexing optimism
  metrics-driven through PIR instead of hard-coding from semantic kind.
  Sources:
  https://research.google/pubs/large-scale-cluster-management-at-google-with-borg/
  and https://www.usenix.org/conference/atc20/presentation/shahrad
- **Serverless locality and reuse:** SAND shows the value of locality-aware,
  low-latency serverless execution, but only after isolation and communication
  semantics are explicit. REC should use locality/warm reuse as an optimization
  over the pool authority key, never as an authority shortcut.
  Source: https://www.usenix.org/conference/atc18/presentation/akkus
- **Strong isolation tier boundary:** Firecracker's serverless isolation work is
  evidence that profile/substrate efficiency is separate from isolation tier.
  REC must not let `RuntimeProfile` stand in for VM/container/isolate trust
  decisions.
  Source: https://www.usenix.org/conference/nsdi20/presentation/agache
- **Side-channel posture for isolates:** Dynamic Process Isolation for V8
  isolates shows that shared-process isolate co-location still needs a
  side-channel policy. REC currently names side-channel posture; REC0 must define
  the concrete inputs or mark cooperative reuse ineligible when they are unknown.
  Source: https://arxiv.org/abs/2110.04751
- **Sequential request isolation under warm reuse:** Groundhog-style attacks
  show that reusing a runtime across sequential requests can leak state unless
  the runtime proves reset/cleanup and authority partitioning. REC's
  `RuntimePoolAuthorityKey` must include all state-bearing inputs or force a
  fresh context/isolate.
  Source: https://arxiv.org/abs/2302.05017
- **JavaScript compartments and endowments:** SES and LavaMoat use hardened
  intrinsics, compartment-specific globals, and explicit endowments/policy files
  to remove ambient authority from JavaScript code. REC does not need to adopt
  SES, but runtime-context narrowing must account for `globalThis`, mutable
  primordials, dynamic import/module graph authority, timers, random, and
  package-provided platform access. A typed `ctx` object alone is not enough
  proof of authority narrowing.
  Sources: https://github.com/endojs/endo/tree/master/packages/ses and
  https://github.com/LavaMoat/LavaMoat
- **Request-local async provenance:** Workerd's `IoContext` and promise
  cross-context tagging show that cooperative interleaving must preserve the
  request/session identity of promises, callbacks, and I/O-owned objects. REC
  must prove a parked invocation cannot resume host resources or callbacks under
  another invocation's host-bridge session.
  Local sources:
  `/Users/jack/src/github.com/cloudflare/workerd/src/workerd/io/worker.c++`
  and `/Users/jack/src/github.com/cloudflare/workerd/src/workerd/io/io-context.*`
- **Input/output gate semantics for observable state:** Workerd actors separate
  input gates, which block interleaving while storage operations are outstanding,
  from output gates, which prevent the outside world from observing writes before
  they are durable. REC should keep write/effect classification separate from the
  lifecycle gates that may be needed to preserve state consistency and
  observability.
  Local source:
  `/Users/jack/src/github.com/cloudflare/workerd/src/workerd/io/io-gate.h`
- **Deterministic versus observable host effects:** Temporal keeps replayed
  workflow code deterministic and routes external interactions through
  Activities or replay-safe SDK APIs. REC should treat time, randomness,
  environment/secrets, auth identity, metadata reads, external reads, and nested
  runtime calls as observable effects unless Nimbus virtualizes or records them
  through an explicit host API.
  Source: https://docs.temporal.io/workflow-definition
- **WASI capability handles and resources:** WASI starts code with no ambient
  authority, grants capabilities at invocation/world boundaries, and represents
  filesystem access through descriptors/resources with read/write/mutate flags.
  REC should align future NimbusFS, object storage, and egress bindings with
  capability handles/resources feeding the host capability set, rather than
  expanding ad hoc global host operations.
  Sources: https://wasi.dev/security and
  https://component-model.bytecodealliance.org/design/wit.html
- **Overload control and sheddability:** Google's SRE guidance treats adaptive
  throttling and request criticality as first-class overload controls; criticality
  is orthogonal to latency and propagates through downstream calls. REC should
  represent queue rejection, shedding, and scheduling priority as explicit
  admission outcomes or internal policy evidence, not as implicit consequences
  of `query`/`mutation`/`action` labels.
  Source: https://sre.google/sre-book/handling-overload/
- **Operational isolation triggers:** Cloudflare Workers documents isolate
  scheduling, private-process escalation for riskier features such as debugging,
  Spectre defense, and trust cordons. REC should treat inspector/debug surfaces,
  privileged runtime features, and trust tier as isolation-placement inputs,
  not merely host-effect subclasses.
  Source: https://developers.cloudflare.com/workers/reference/security-model/

Open validation questions for REC0:

1. **Side-channel posture inputs:** What concrete facts make shared-process
   isolate co-location safe enough for cooperative reuse? REC0 must define the
   inputs, including high-resolution timers, shared memory, native/FFI/process
   surfaces, Node-compatible built-ins, inspector/debug surfaces, external
   service grants, and profile/isolation tier. Unknown posture means ineligible.
2. **Pool authority key completeness:** Which state-bearing facts invalidate
   warm reuse? REC0 must check tenant/principal, runtime profile, permission
   profile, exact service grants, bundle/provenance hash, environment/secrets
   version, generated context shape, module graph/cache key, Node compatibility
   surface, and host bridge session identity. Missing facts force fresh context
   or fresh isolate.
3. **Read-only versus observable:** Are all read host operations equally safe?
   REC0 must split at least pure/local reads from observable reads such as auth
   identity, environment variables, file/storage metadata, scheduler metadata,
   time/random, and external service reads. Observable reads may still be
   non-mutating, but they are not automatically cooperative-safe.
4. **Nested runtime and function-call matrix:** Convex permits only specific
   nested query/mutation relationships. REC0 must decide whether Nimbus's
   matrix is identical, intentionally narrower, or needs additional host-effect
   states for snapshot query, query journal, and nested runtime calls.
5. **NodeFull cooperative eligibility:** The plan assumes a future path where
   NodeFull read-only queries may remain eligible when side-channel posture and
   effects are safe. REC0 must validate this against Node built-ins and Nimbus
   bootstrap surfaces; otherwise NodeFull starts ineligible by default.
6. **Codegen/context narrowing feasibility:** Generated reader-only contexts are
   desirable, but REC0 must validate whether the current JS SDK/Convex wrapper
   can expose narrower runtime objects without duplicating parallel logic or
   creating type/runtime disagreement.
7. **Effect-class exhaustiveness granularity:** REC2 says every
   `HostCallOperation` is classified. REC0 must decide whether the enum needs
   sub-operations or parameters for service/external reads, storage URL reads,
   scheduler reads, extension calls, and host-observed side effects.
8. **Numeric thresholds for scheduling class:** CPU-heavy read-only work may be
   semantically safe but unfair to multiplex. REC0 must bind the thresholds to
   PIR metrics and avoid hard-coded assumptions from semantic kind.
9. **Failure-mode contract:** What happens when metadata and runtime observation
   disagree? REC0 must define the error/telemetry path for generated metadata
   that says read-only while host-op observation sees a write, service, nested
   runtime, external, or unknown effect.
10. **Isolation-tier escape hatches:** Native process, FFI, inspector, or
    privileged service surfaces should probably promote the required isolation
    tier instead of merely changing effect class. REC0 must validate this with
    `auth-runtime-trust.md`, PIR, and sandbox plans before implementation.
11. **JavaScript global authority posture:** Can generated query contexts be
    narrowed without leaving equivalent authority through `globalThis`,
    primordials/prototype mutation, dynamic import, module cache state,
    package-provided Node/Web APIs, timers, or randomness? If not, REC4 must
    record context narrowing as partial evidence only and keep host-op
    enforcement/fresh context as the trust boundary.
12. **Async context provenance:** What prevents a promise, callback, stream, or
    host-owned object created by invocation A from being resolved or resumed
    under invocation B's host bridge session during cooperative interleaving?
    REC0 must either point to an existing provenance guard or require one before
    REC3 enables plan-based interleaving.
13. **Observable output gates:** For writes, scheduler changes, service calls,
    and nested runtime effects, does Nimbus need a lifecycle gate so external
    observers cannot see results before durable storage/journal effects are
    committed? If existing engine/storage atomicity is enough, REC0 must cite the
    exact code path; otherwise effectful work remains run-to-completion.
14. **Determinism and virtualized reads:** If REC wants a "read-only" class to be
    replayable, cacheable, or snapshot-safe, which host reads are virtualized or
    recorded? Time, random, env/secrets, auth identity, file/storage metadata,
    external reads, and scheduler metadata should default to observable effects
    until an explicit replay-safe API proves otherwise.
15. **Overload and criticality source:** Does Nimbus have an internal source of
    scheduling criticality, sheddability, or priority distinct from semantic
    function kind? If not, REC must avoid inventing per-kind criticality and
    instead emit explicit queue/pool/host-pressure rejection telemetry using the
    existing tenant and PIR7 budget facts.
16. **Future capability-resource alignment:** How will future NimbusFS, object
    storage, service bindings, and egress gateway capabilities appear in REC?
    REC0 must decide whether they are represented as typed capability handles,
    effect subclasses, pool-authority-key inputs, or isolation-tier inputs before
    adding global host operations that would need to be unwound later.

REC adopts these canonical names and boundaries:

- **Semantic kind / invocation kind**: Convex contract only.
- **Runtime surface / profile**: substrate and startup shape only.
- **Host capability set**: which host operations are present in context.
- **Host effect class**: pure/local read, observable read, write, scheduler,
  service/external, nested runtime, extension, unknown.
- **Pool authority key**: tenant/principal/profile/grant/bundle facts that make
  reuse safe.
- **Async context provenance**: request/session identity carried by promises,
  callbacks, streams, and host-owned resources across cooperative yield/resume.
- **Scheduler admission / cooperative eligibility**: derived, fail-closed output
  consumed by the worker loop.
- **Admission outcome / shedding reason**: explicit queue, pool, tenant-budget,
  or host-pressure decision; not a semantic-kind alias.
- **Runtime enforcement**: CPU, memory, subrequest, queue, timeout, and
  termination guards that enforce the selected plan.

REC explicitly rejects:

- A monolithic `RuntimeExecutionClass` that directly implies tenant authority,
  host capabilities, pool placement, and scheduler behavior.
- Using codegen metadata, developer-authored hints, or `Query` naming as a
  trust boundary.
- Duplicating PIR7 host-work admission instead of consuming
  `RuntimeHostWorkClass` / `RuntimeHostResourceDecision`.

### Required Ownership

| Module | Interface / seam | Locality rule | Required proof |
|---|---|---|---|
| `nimbus-runtime` | `RuntimeExecutionPlan`, `RuntimeEffectClass`, `CooperativeEligibility`, host-call operation classification | Owns runtime-local execution facts without depending on workspace crates. It may classify `HostCallOperation`, request kind, bundle identity, limits, and metrics snapshots, but it must not know Convex storage internals. | Unit tests for pure classification, no-op baseline, and scheduler use of the plan instead of direct `InvocationKind` checks. |
| `nimbus-tenant` | Admission and tenant policy facts consumed by the execution plan | Owns tenant/isolation/admission facts. It does not own pool sizing, cooperative eligibility, host work class, or profile constants. The current `RuntimeEfficiencyPlan` must be narrowed/retired or treated only as admission evidence before REC scheduler policy grows. | Tests proving an execution plan cannot downgrade admission, isolation tier, permission profile, service grants, or resource caps. |
| `nimbus-server` | Adapter lowering from Convex invocation/context/service facts into runtime execution facts | Owns adapter-specific effect knowledge such as generated query handlers vs runtime handlers, service lookup, scheduler, and external adapter grants. | Integration tests proving query-shaped but effectful code is not cooperatively multiplexed. |
| `packages/codegen` | Runtime handler metadata and context-shape hints | May emit static metadata about required surface/effects when it can prove them. It must not be the only enforcement layer. | Codegen tests for read-only query context, mutation writer context, action/effectful metadata, and conservative fallback for dynamic handlers. |
| PIR benchmark harness | Numeric validation | Supplies before/after timings, RSS, CPU/yield mix, queue/occupancy, host work-class pressure, and crossover data. | Benchmark artifacts comparing current PIR baseline to REC after each scheduling-affecting band. |

### Non-negotiable Invariants

- `InvocationKind` must not directly decide pool size, worker pinning, snapshot
  choice, CPU fairness, host isolation tier, or tenant quota.
- Cooperative multiplexing requires `function_kind` plus `effect_class` plus
  side-channel posture. A bare `Query` label is insufficient.
- Worker routing affinity is not authority. `RuntimeAffinityKey` may improve
  locality, but only `RuntimePoolAuthorityKey` may decide warm reuse or context
  sharing.
- `RuntimeProfile` remains an efficiency/substrate label only. It never changes
  tenant admission, isolation tier, permission profile, exact service grants, or
  resource caps.
- Runtime context capability narrowing is preferred when possible; host-op
  enforcement remains mandatory.
- Missing effect evidence fails closed for cooperative eligibility. Unknown or
  dynamic runtime handlers start as `EffectUnknown`/ineligible until static
  metadata or host-op enforcement proves a narrower class.
- Pool acquisition uses profile plus authority key. Autoscaling and warm-count
  sizing use metrics and budgets.
- Cooperative interleaving must preserve async context provenance. Promises,
  callbacks, streams, host resources, and bridge sessions created for one
  invocation must not resume under another invocation's session.
- Context narrowing must account for JavaScript ambient authority outside the
  `ctx` object, including globals, primordials, module cache/import authority,
  timers, randomness, and Node/Web platform APIs.
- Overload admission must return an explicit rejection/shedding reason and
  telemetry. It must not silently downgrade isolation, broaden pool reuse, or
  reclassify effects to create capacity.
- No developer-facing or tenant-authored runtime efficiency knobs are added.

---

## Phase Status Ledger

| Band | Status | Summary | Hard Deps | Gate Note |
|---|---|---|---|---|
| REC0 | `done` | Baseline audit and scaffold: copy the classifier design, canonical exemplar audit, full architecture audit, complexity pockets, Rust idiom contract, and open validation answers into durable proof. Inventory every current place where `InvocationKind`, `RuntimeProfile`, `RuntimeEfficiencyPlan`, `RuntimeTenantBudget`, `RuntimeHostWorkClass`, `RuntimeHostResourceDecision`, host-call ops, `RuntimeAffinityKey`, pool reuse, and scheduler eligibility are used. Create `scripts/verify-runtime-execution-classification.sh` with pre-existence checks. | PIR0, PIR1, PIR4, PIR7 proof artifacts; `bash scripts/verify-profile-aware-isolate-runtime.sh` green | Current-state diagram, canonical-pattern comparison, complexity-pocket matrix, validation-question answers, and every direct `allows_cooperative_multiplexing` consumer are recorded in `docs/private/plans/proof/runtime-execution-classification/rec0-baseline.md`. |
| REC1 | `done` | Introduce internal classifier types: `RuntimeEffectClass`, `CooperativeEligibility`, `CooperativeIneligibilityReason`, `RuntimeSchedulingClass`, `RuntimePoolAuthorityKey`, `RuntimeAdmissionOutcome`, and `RuntimeExecutionPlan`. Keep public API unchanged. | REC0 | Classification is pure, deterministic, cheap, typed, and unit-tested with fake metrics/budgets. Focused proof is in `docs/private/plans/proof/runtime-execution-classification/rec1-execution-plan.md`. |
| REC2 | `done` | Classify `HostCallOperation` effects and wire observed host-op enforcement to the execution plan. Read-only, observable-read, write, scheduler, service/external, nested runtime, HTTP route, and extension calls are explicit. | REC1 | Query-shaped code that reaches write/service/scheduler/nested/external/unknown ops is denied or marked effectful before cooperative reuse can continue. Focused proof is in `docs/private/plans/proof/runtime-execution-classification/rec2-host-effects.md`. |
| REC3 | `done` | Replace worker-loop direct `InvocationKind::is_convex_read_semantic_candidate` checks with `RuntimeExecutionPlan::cooperative_eligibility`. Preserve PIR4 current behavior for proven read-only queries. | REC1, REC2, PIR3 | Existing cooperative tests still pass; every scheduler/pool/autoscale decision stops consuming `InvocationKind` directly; new effectful-query and async-provenance tests prove fail-closed behavior. Focused proof is in `docs/private/plans/proof/runtime-execution-classification/rec3-scheduler-consumption.md`. |
| REC4 | `done` | Narrow runtime context capabilities and codegen metadata where practical. Generated queries receive reader-only context or conservative effect metadata; dynamic handlers remain fail-closed unless proven. | REC2, REC3 | Type-level intent, runtime object shape, JS ambient authority posture, and host-op enforcement no longer contradict each other for generated handlers. Focused proof is in `docs/private/plans/proof/runtime-execution-classification/rec4-context-codegen-alignment.md`. |
| REC5 | `done` | Numeric validation and closeout: rerun PIR benchmark lanes before/after REC, record performance deltas, update PIR linkage, and decide whether any classifier overhead needs hot-path optimization. | REC3, REC4 | Safety gates remain green. Run-to-completion and retained RSS lanes are acceptable, but WebStandard cooperative warm-pool latency is recorded as a measured exception in `docs/private/plans/proof/runtime-execution-classification/rec5-numeric-closeout.md`; broader cooperative defaults must optimize that path first. |

---

## Implementation Checkpoints

### REC0 - Baseline Audit and Verifier Scaffold

- Inventory these symbols and paths:
  - `InvocationKind::is_convex_read_semantic_candidate`
  - `worker_loop/cooperative::{run,execution}.rs`
  - `executor/{queue/job,invoke,admission}.rs`
  - `RuntimeEfficiencyPlan`
  - `crates/nimbus-runtime/src/limits/profile.rs` and `RuntimeProfile::for_limits`
  - `RuntimeTenantBudget`
  - `RuntimeHostWorkClass` and `RuntimeHostResourceDecision`
  - `RuntimeAffinityKey` and worker router affinity
  - `HostCallOperation` and `HostCallPayload`
  - `HostCallEnvelope` and `HostCallPayload::operation`
  - Convex host bridge dispatch in `adapters/convex/host_bridge/*`
  - `crates/nimbus-bridge/src/state.rs` host-call session validation
  - Convex runtime context creation in `runtime/bootstrap/source.rs`
  - `packages/codegen/src/emit/runtime_bundle_*`
  - `packages/codegen/src/planner/context_api.mjs`
  - Convex host bridge document, scheduler, service, nested-runtime dispatch
- Record whether each use is semantic, substrate/profile, effect/capability,
  scheduling, pooling, or telemetry.
- Record the complexity-pocket owner for each use: scheduler predicate spread,
  job/admission coupling, host-call ABI/adapter dispatch, routing-locality versus
  authority reuse, tenant-admission versus runtime efficiency, context narrowing
  versus JS ambient authority, telemetry cardinality, or verifier drift.
- Copy the Canonical Exemplar Audit into the REC0 proof artifact with exact
  local source paths inspected. If a future implementer clones another exemplar
  into `/Users/jack/src/github.com/<org>/<repo>`, record why it changed the
  design; otherwise record that no stronger exemplar was needed.
- Answer or explicitly defer every Open Validation Question in the REC0 proof
  artifact. Any deferred answer must choose the safer behavior for
  implementation, such as ineligible cooperative reuse, fresh context/isolate,
  effect unknown, or higher isolation tier.
- Create proof artifact:
  `docs/private/plans/proof/runtime-execution-classification/rec0-baseline.md`.
- Create verifier scaffold:
  `scripts/verify-runtime-execution-classification.sh`.

### REC1 - Derived Execution Plan Module

- Add an internal classifier module with a small interface and pure tests.
- Keep `InvocationKind` as the semantic function kind.
- Replace the current scheduler-specific method with a name that cannot be
  mistaken for pool placement, for example:
  `InvocationKind::is_convex_read_semantic_candidate`.
- Keep the classifier module deep: the worker loop should consume
  `RuntimeExecutionPlan` and ineligibility/admission reasons without knowing how
  semantic kind, runtime profile, host effects, authority key, side-channel
  posture, and budgets were combined.
- Use typed closed enums/newtypes for classifier outputs. Avoid stringly reason
  fields except at telemetry/diagnostic serialization edges.
- Do not reuse `RuntimeAffinityKey` as `RuntimePoolAuthorityKey`. Routing
  affinity may be an input, but authority reuse must be exact and fail-closed.
- Reconcile `nimbus-tenant::RuntimeEfficiencyPlan`: either narrow it into
  admission/profile evidence consumed by `RuntimeExecutionPlan`, or remove it
  after moving the runtime-owned decision facts into the new REC module. Do not
  let it grow scheduler, host work-class, pool sizing, or cooperative admission
  policy.
- The execution plan carries ineligibility reasons such as:
  `EffectfulKind`, `UnknownEffect`, `WriteHostOperation`, `ServiceGrant`,
  `SchedulerOperation`, `ExternalNetwork`, `SideChannelPostureMissing`,
  `UnsupportedRuntimeSurface`, and `OperatorDisabled`.

### REC2 - Host Operation Effect Classifier

- Classify every `HostCallOperation`.
- Make the classifier exhaustive over the `HostCallOperation` enum, not a
  string table. Adding a new host operation without an explicit effect class
  must fail tests or the REC verifier.
- Classify effect granularity precisely enough for REC0's read-only versus
  observable-read decision. Reads of time, random, auth identity, env/secrets,
  filesystem/storage metadata, scheduler metadata, service discovery, external
  state, or adapter extension state are not automatically pure read effects.
- Fail closed for operations not explicitly classified.
- Deny or mark effectful when a cooperative-eligible invocation attempts:
  - `DocumentInsert`, `DocumentPatch`, `DocumentDelete`
  - scheduler operations
  - service lookup
  - nested mutation/action/runtime calls
  - adapter extension calls that do not declare read-only behavior
- Keep host-call session validation and tenant principal checks unchanged.
- Tests must prove the host bridge sees the original session/principal and that
  effect violations are typed errors, not string-only contract failures.
- Tests must prove metadata and runtime observation conflicts cannot silently
  continue as cooperative-eligible work.

### REC3 - Scheduler Consumption

- `RuntimeWorkerJob` or its admission context carries a `RuntimeExecutionPlan`.
- Cooperative worker admission consumes `CooperativeEligibility`.
- Host-pressure admission and scheduling class consume/extend PIR7's existing
  `RuntimeHostWorkClass` / `RuntimeHostResourceDecision` model. REC must not add
  a second host-admission hierarchy beside PIR7.
- Worker routing may continue using `RuntimeAffinityKey`, but pool reuse and
  cooperative context sharing must use `RuntimePoolAuthorityKey`.
- Admission outcomes must distinguish tenant queueing, host-pressure queueing,
  host-pressure shedding, classifier ineligibility, and cancellation in typed
  metrics/errors.
- Non-read-safe work waits behind live cooperative slots exactly as PIR4 proved,
  but the reason is now the plan, not direct request kind checks.
- Retain the PIR4 guarantee: mutations/actions remain direct run-to-completion.
- Add tests for:
  - WebLean read-only query remains eligible
  - NodeFull read-only query remains eligible only when side-channel posture and
    effect metadata are safe
  - query-shaped write is ineligible
  - query-shaped service lookup is ineligible
  - action/mutation remain ineligible
  - CPU-heavy read-only work may be read-safe but receives a scheduling class
    that reduces multiplexing optimism and charges tenant/host CPU budgets

### REC4 - Runtime Context and Codegen Alignment

- Generated query handlers should receive a reader-only runtime context where
  the codegen/runtime can enforce it.
- Generated query handlers receive the Convex `runQuery` nested-call surface but
  not `runMutation` or `runAction`.
- Mutation handlers receive writer/scheduler context plus `runQuery` and
  `runMutation`, but not `runAction`.
- Action handlers receive action context and the full Convex nested-call surface.
- Dynamic runtime handlers that cannot be statically classified are conservative:
  ineligible for cooperative multiplexing unless a narrower metadata proof exists.
- Codegen metadata may improve the plan, but host-op enforcement remains the
  trust boundary.
- Generated/static metadata must be represented as an input to REC, not as a
  scheduler bypass. A query handler with reader-only metadata still becomes
  ineligible if runtime host-op observation reports write, service, scheduler,
  nested runtime, external, or unknown effects.
- Runtime context narrowing must account for global/endowment authority, not
  only the shape of the `ctx` parameter. If JavaScript globals, primordials,
  dynamic imports, module cache entries, timers, random, or Node/Web APIs expose
  equivalent authority, REC4 records the context as only partially narrowed and
  keeps cooperative eligibility behind host-op enforcement plus fresh
  context/isolate proof.

### REC5 - Numeric Validation

Use PIR's benchmark harness and methodology. Do not invent a second benchmark
format.

Minimum before/after lanes:

- WebStandard read-only hostless query
- WebStandard synthetic-await read-only query, cooperative warm-pool lane
- NodeFull hostless query
- NodeFull synthetic-await read-only query
- CPU-bound read-only query
- mutation direct run-to-completion
- action/direct effectful work
- query-shaped write/service/scheduler negative cases
- cross-invocation async provenance negative case, if REC3 introduces or relies
  on cooperative yield/resume across multiple live invocations

Minimum metrics:

- median and p95 invocation latency
- current RSS / retained pool RSS where applicable
- worker occupancy and queued time
- runtime pool hits/misses/replacements
- cooperative parked/resumed counts
- host-call operation counts by effect class
- queue/pool/host-pressure rejection counts by admission outcome or shedding
  reason
- CPU-time or proxy CPU budget charge where available
- PIR7 host work-class and host-pressure admission decisions

Numeric success threshold:

- For the primary WebStandard and NodeFull read-only lanes, median overhead from
  classification must be <= 5% versus the PIR baseline, or REC5 must record the
  measured exception and the optimization plan before defaulting the new path.
- p95 overhead must be <= 10% for read-only cooperative lanes unless the variance
  is already present in the PIR baseline and the trace explains it.
- Current RSS must not grow by more than measurement noise for classification-only
  changes.
- Effectful-query negative cases must be 100% denied or marked ineligible before
  cooperative admission.
- No pool/autoscaling decision may use `InvocationKind` directly after REC3.
- No host admission decision may fork PIR7's `RuntimeHostWorkClass` /
  `RuntimeHostResourceDecision` model.

---

## Band-level Proof Matrix

| Band | Behavioral success criteria | Architecture / maintainability success criteria | Verifier evidence |
|---|---|---|---|
| REC0 | Current classifier usage inventory is complete and reconciled with code. | The plan names the future deep module, canonical exemplar comparison, open validation answers, and deletion-test result. | Verifier checks proof file, current-state inventory, exemplar audit carry-forward, validation-question coverage, and direct scheduler consumers. |
| REC1 | Pure classifier maps known semantic/profile/effect/authority/budget inputs to expected execution plans. | `InvocationKind` loses scheduler-specific ownership; new module has a small interface, typed Rust outputs, and follows the canonical multi-axis plan pattern. | Unit tests plus verifier checks for classifier types, canonical boundary names, no public config knobs, and no `RuntimeAffinityKey` authority reuse. |
| REC2 | Every host operation has an effect class; unknown/effectful/observable-conflict ops fail closed for cooperative eligibility. | Host-op effect classification is centralized, typed, and close to `HostCallOperation`. | Host bridge tests, operation-classifier tests, metadata-vs-observed-effect conflict tests, verifier checks no unclassified operation exists. |
| REC3 | Scheduler consumes execution-plan eligibility; existing PIR4 read-safe interleaving still works. | Worker loop no longer calls `InvocationKind` for scheduling policy and does not duplicate PIR7 host-admission hierarchy. | Cooperative tests, mutation/action exclusion tests, effectful-query negative tests, async provenance tests, `rg` verifier for direct scheduler consumers. |
| REC4 | Runtime context shape or metadata matches query/mutation/action capabilities; dynamic handlers stay conservative. | Codegen metadata improves classification but is not the trust boundary; JS ambient authority posture is recorded. | Codegen tests, runtime context tests, global-authority regression tests where practical, host-op enforcement tests. |
| REC5 | PIR benchmark lanes prove overhead and RSS thresholds, or record measured exceptions. | Numeric validation follows PIR methodology and feeds future PIR default decisions; closeout proof covers ledger/verifier drift. | Benchmark artifacts, proof update, verifier checks for artifact paths, threshold summary, and no direct `InvocationKind` scheduling path. |

Verifier gate: `bash scripts/verify-runtime-execution-classification.sh`.

---

## Autonomous `/goal` Prompt

```text
/goal Autonomously complete the entire docs/private/plans/runtime-execution-classification-plan.md implementation in /Users/jack/src/github.com/nimbus/nimbus. The objective is not REC0, a scaffold, or one selected band; the objective is REC0, REC1, REC2, REC3, REC4, and REC5 all marked done with proof artifacts, verifier coverage, tests, benchmarks where required, and exact verification output recorded. Treat docs/private/plans/profile-aware-isolate-runtime-plan.md as the benchmark/profiling foundation and do not invent a second numeric methodology. Keep exactly one REC band in_progress at a time, but after a band is verified and marked done, immediately promote the next eligible band and continue in the same autonomous run. Preserve PIR invariants: RuntimeProfile is efficiency only, tenant admission and grants remain authoritative, nimbus-runtime has zero workspace dependencies, nimbus-core has zero I/O, and no developer-facing runtime efficiency knobs are added.

Rules:
- Read README.md, ARCHITECTURE.md, docs/README.md, docs/private/plans/README.md, docs/private/plans/profile-aware-isolate-runtime-plan.md, docs/private/plans/runtime-execution-classification-plan.md, docs/private/architecture/runtime/adapter-boundary.md, docs/private/architecture/server/auth-runtime-trust.md, and the code files named by the active REC band before editing.
- Run or inspect `bash scripts/verify-profile-aware-isolate-runtime.sh` first and record the exact pass/fail count. The audited baseline on 2026-06-21 was 90 passed, 0 failed.
- Inspect git status first. Do not revert unrelated changes.
- Start with the ledger's active REC band. If none is active, promote REC0 only.
- Do not stop after REC0 scaffold work, after creating the verifier, after a proof artifact, or after any single band. A band closeout is a checkpoint, not completion. Continue through the next eligible REC band until REC0..REC5 are complete or a blocker is recorded with exact evidence and the safer fail-closed behavior.
- Only three stopping states are allowed: (1) REC0..REC5 are all done, `bash scripts/verify-runtime-execution-classification.sh` and the relevant test/benchmark gates are green, and the final plan/proof closeout is recorded; (2) a real blocker prevents further progress after local recovery attempts, with the blocker, failed command output, fail-closed interim behavior, and next unblock action recorded in this plan; or (3) the user explicitly interrupts and asks to stop.
- Build the internal RuntimeExecutionPlan seam so semantic kind, runtime profile, effect class, cooperative eligibility, pool authority, scheduling class, and budgets remain separate facts.
- Preserve the canonical exemplar audit: do not collapse semantic kind, host capability/effect, pool authority, scheduler admission, and runtime enforcement into a monolithic class enum.
- In REC0, answer every Open Validation Question before REC1. If an answer is not yet provable, choose the fail-closed behavior in the implementation plan and record the missing proof.
- Carry forward the second-pass research themes: JS compartment/endowment hardening, async context provenance, input/output gate semantics, deterministic-vs-observable host effects, WASI capability resources, overload/shedding policy, and operational isolation triggers.
- Carry forward the full architecture audit: contain scheduler predicate spread, job/admission coupling, host-call ABI drift, routing-affinity versus authority-key confusion, tenant-admission versus runtime-efficiency leakage, context-narrowing versus JS ambient authority, metrics cardinality, and verifier drift.
- Do not let InvocationKind directly decide pool size, worker pinning, snapshot choice, CPU fairness, host isolation tier, tenant quota, or cooperative scheduler admission after REC3.
- Do not reuse RuntimeAffinityKey as RuntimePoolAuthorityKey. Routing locality is approximate; authority reuse is exact and fail-closed.
- Reconcile `nimbus-tenant::RuntimeEfficiencyPlan` before scheduler policy grows: narrow it into admission/profile evidence or remove it after moving runtime-owned decision facts into REC.
- Make host-operation effect classification exhaustive over the `HostCallOperation` enum and fail closed for new/unclassified operations.
- Consume PIR7's `RuntimeHostWorkClass` / `RuntimeHostResourceDecision` for host pressure and work-class decisions; do not create a parallel host admission hierarchy.
- Use idiomatic Rust seams: closed enums, typed newtypes, `Duration`, `NonZero*` where applicable, pure deterministic classifier functions, `#[must_use]` decision types, and typed errors for classification conflicts/effect violations/admission outcomes.
- Implement tests, proof artifacts, verifier growth, and plan updates together. Do not mark a band done without exact command output/counts and, for scheduling-affecting work, PIR-style benchmark numbers.
- At each band checkpoint, update the Phase Status Ledger, Implementation Checkpoints, Execution Log, proof artifacts, and verifier expectations before promoting the next band. Before final completion, ensure there is no next eligible band, REC0..REC5 are all done, and the verifier/proof closeout says so explicitly.
```

---

## Execution Log

- 2026-06-21 - Created REC plan from the execution-classifier audit. The plan
  keeps PIR as the numeric profiling/benchmarking foundation and routes the
  query/mutation/action scheduling concern into a derived execution-plan module
  instead of overloading `InvocationKind`.
- 2026-06-21 - Readiness audit against the in-flight PIR branch and live runtime
  code. `bash scripts/verify-profile-aware-isolate-runtime.sh` passed with
  `90 passed, 0 failed`. Folded in the current PIR7 host-budget baseline, the
  singular `limits/profile.rs` path, the tenant-owned `RuntimeEfficiencyPlan`
  reconciliation requirement, exhaustive `HostCallOperation` effect
  classification, and the rule that REC consumes PIR7's
  `RuntimeHostWorkClass` / `RuntimeHostResourceDecision` rather than forking a
  host-admission model.
- 2026-06-21 - Canonical exemplar audit across local GitHub worktrees and
  GitHub repository search. Compared Workerd, Convex backend, Deno/deno_core,
  Supabase Edge Runtime, OpenWorkers, Wasmtime, isolated-vm/isolator, Kubernetes,
  and Nomad. The adopted pattern is a multi-axis execution plan with separate
  semantic kind, runtime surface, host capability/effect, pool authority,
  scheduler admission, and runtime enforcement; REC now rejects a monolithic
  execution-class authority enum.
- 2026-06-21 - Added research and validation inputs that were not fully proven
  by implementation exemplars: capability-security posture, fail-closed
  permissions, filter-vs-score scheduling, workload heterogeneity, serverless
  locality/reuse, isolation-tier separation, V8 isolate side channels, and
  sequential warm-reuse leakage. REC0 must answer the validation questions or
  choose fail-closed defaults before REC1 starts.
- 2026-06-21 - Added second-pass research inputs for JavaScript compartment
  endowments, LavaMoat/SES-style hardened globals, Workerd async-context and
  input/output gate patterns, Temporal deterministic workflow constraints, WASI
  capability handles/resources, SRE overload criticality/shedding, and
  Cloudflare operational isolation triggers. REC0 now must validate async
  provenance, observable gates, deterministic-read posture, overload criticality
  source, and future capability-resource alignment before REC1.
- 2026-06-21 - Full architecture audit pass. Added current code audit facts,
  complexity pockets, Rust idiom and resilience contract, explicit routing
  affinity versus pool authority separation, typed classifier/admission
  success criteria, and a stronger `/goal` prompt that drives REC0 through REC5
  instead of stopping after a scaffold band.
- 2026-06-21 - Tightened the autonomous `/goal` prompt so the objective is the
  entire REC0-through-REC5 plan, not one band. The prompt now defines allowed
  stopping states, treats band closeout as a checkpoint, and requires immediate
  promotion of the next eligible band until full plan closeout or a recorded
  blocker.
- 2026-06-21 - Runtime plan-family alignment pass. Refreshed the PIR verifier
  baseline to 90 passed, 0 failed and made REC the hard predecessor for
  NodeFull realm-lease implementation phases NFR2-NFR6. NFR may consume REC's
  execution-plan, authority-key, effect-class, async-provenance, and PIR7
  host-governance vocabulary after REC closeout; it must not grow a parallel
  scheduler or authority model.
- 2026-06-21 - REC0 baseline audit closeout: added
  `docs/private/plans/proof/runtime-execution-classification/rec0-baseline.md`
  and `scripts/verify-runtime-execution-classification.sh`. The proof records
  the current-state diagram, required symbol inventory, direct
  `InvocationKind::allows_cooperative_multiplexing` consumers, host operation
  inventory, canonical pattern carry-forward, complexity-pocket matrix, and all
  OVQ-01 through OVQ-16 validation answers with conservative defaults. Promoted
  REC1 to `in_progress`; REC1 must introduce the typed internal execution-plan
  module without public API changes.
- 2026-06-21 - REC1 internal execution-plan vocabulary closeout: added
  `crates/nimbus-runtime/src/execution_plan.rs` with internal closed types for
  `RuntimeEffectClass`, `RuntimeSideChannelPosture`, `CooperativeEligibility`,
  `CooperativeIneligibilityReason`, `RuntimeSchedulingClass`,
  `RuntimePoolAuthorityKey`, `RuntimeAdmissionOutcome`, and
  `RuntimeExecutionPlan`. Renamed the current `InvocationKind` helper to
  `is_convex_read_semantic_candidate` so it records semantic query candidacy
  only, not scheduler authority. Focused verification
  `cargo test -p nimbus-runtime runtime_execution_plan --lib -- --nocapture`
  passed with 5 passed, 0 failed, 0 ignored, 0 measured, 1050 filtered out.
  Promoted REC2 to `in_progress`; REC2 must classify every `HostCallOperation`
  exhaustively and wire observed host effects to the plan.
- 2026-06-21 - REC2 host-operation effect closeout: added exhaustive
  `HostCallOperation::runtime_effect_class`, typed
  `RuntimeObservedEffectViolation`, per-invocation
  `RuntimeInvocationExecutionPlanBinding`, and shared async/sync host-call
  effect enforcement. The binding is inactive until REC3 installs execution
  plans, preserving current runtime behavior while making cooperative-eligible
  pure-local plans fail closed if they observe query, write, scheduler, service,
  nested-runtime, HTTP, extension, or unknown host effects. Focused verification
  `cargo test -p nimbus-runtime runtime_execution_plan --lib -- --nocapture`
  passed with 6 passed, 0 failed, 0 ignored, 0 measured, 1051 filtered out;
  `cargo test -p nimbus-runtime host_call_operations_have_exhaustive_runtime_effect_classes --lib -- --nocapture`
  passed with 1 passed, 0 failed, 0 ignored, 0 measured, 1056 filtered out.
  Promoted REC3 to `in_progress`; REC3 must install and consume
  `RuntimeExecutionPlan` in worker scheduling/admission instead of direct
  semantic-kind checks.
- 2026-06-21 - REC3 scheduler-consumption closeout: added
  `RuntimeExecutionPlan::for_invocation`,
  `RuntimeExecutionPlan::permits_cooperative_scheduler_admission`, carried
  `RuntimeExecutionPlan` on `RuntimeWorkerJob` and
  `RuntimeInvocationExecution`, installed the plan into invocation op state,
  and moved cooperative worker-loop admission plus host work-class admission to
  consume the plan. `InvocationKind::is_convex_read_semantic_candidate` remains
  only as classifier input, not scheduler policy. REC3 reconciled REC1's
  conservative observable-read posture with PIR4 compatibility: `ObservableRead`
  remains eligible only when semantic kind, WebLean profile, side-channel
  posture, pool authority, scheduling class, and operator gate are safe.
  Focused verification
  `cargo test -p nimbus-runtime runtime_execution_plan --lib -- --nocapture`
  passed with 12 passed, 0 failed, 0 ignored, 0 measured, 1051 filtered out;
  `cargo test -p nimbus-runtime cooperative_execution_model --lib -- --nocapture`
  passed with 4 passed, 0 failed, 0 ignored, 0 measured, 1059 filtered out;
  `cargo test -p nimbus-runtime rec3_query_write_effect_violation_rejects_before_host_dispatch --lib -- --nocapture`
  passed with 1 passed, 0 failed, 1 ignored, 0 measured, 1063 filtered out;
  `cargo test -p nimbus-runtime pir4_mutations_do_not_enter_multiplexed_read_safe_scheduler --lib -- --nocapture`
  passed with 1 passed, 0 failed, 1 ignored, 0 measured, 1063 filtered out.
  Promoted REC4 to `in_progress`; REC4 must align runtime context shape and
  codegen metadata with the execution plan while keeping host-op observation as
  the trust boundary.
- 2026-06-21 - REC4 runtime-context and codegen-alignment closeout: runtime
  bootstrap `__nimbusCreateContext({ request })` now derives a request-kind
  capability shape. 2026-06-27 closeout corrected this to an explicit
  `nestedCalls` matrix that matches the Convex contract: query/paginated-query
  contexts expose reader-only `db`, deny scheduler, and allow only `runQuery`;
  mutation contexts expose writer `db`, scheduler, `runQuery`, and
  `runMutation`, but deny `runAction`; actions and HTTP actions expose
  scheduler plus `runQuery`, `runMutation`, and `runAction` without direct `db`.
  Request-less dynamic/internal contexts retain the conservative legacy surface.
  Codegen context metadata now matches the same query/mutation/action capability
  split, while REC2/REC3 host-op observation remains the trust boundary for raw
  host-op bypasses. Focused verification
  `cargo test -p nimbus-runtime runtime_query_context_is_reader_only_when_request_kind_is_present --lib -- --nocapture`
  passed with 1 passed, 0 failed, 0 ignored, 0 measured, 1066 filtered out;
  `cargo test -p nimbus-runtime runtime_mutation_context_exposes_query_and_mutation_nested_calls --lib -- --nocapture`
  passed with 1 passed, 0 failed, 0 ignored, 0 measured, 1232 filtered out;
  `cargo test -p nimbus-runtime runtime_action_context_exposes_nested_calls_without_direct_db --lib -- --nocapture`
  passed with 1 passed, 0 failed, 0 ignored, 0 measured, 1232 filtered out;
  `cargo test -p nimbus-runtime rec3_query_write_effect_violation_rejects_before_host_dispatch --lib -- --nocapture`
  passed with 1 passed, 0 failed, 1 ignored, 0 measured, 1065 filtered out;
  `npm run test --workspace @nimbus/codegen` passed with runtime remap fixtures
  ok across 4 cases and a direct Convex nested-call matrix selftest. Promoted
  REC5 to `in_progress`; REC5 must run PIR-aligned numeric validation and
  closeout checks before the REC plan can be marked done.
- 2026-06-21 - REC5 numeric validation and closeout: reused the PIR Criterion
  harness and JSONL trace methodology. Added a per-invocation
  `RuntimeWaitUntilState` fast-op marker so no-waitUntil invocations skip the
  waitUntil phase, and preserved PIR2's centralized warm-runtime cleanliness
  gate before runtime retention. `cargo test -p nimbus-runtime pir4_wait_until --lib -- --nocapture`
  passed with 4 passed, 0 failed, 0 ignored, 0 measured, 1063 filtered out;
  `cargo test -p nimbus-runtime warm_pool --lib -- --nocapture` passed with 19
  passed, 0 failed, 9 ignored, 0 measured, 1039 filtered out. Selected REC5
  benchmark artifacts are in
  `docs/private/plans/proof/runtime-execution-classification/artifacts/`.
  WebStandard hostless run-to-completion stayed within threshold at +0.89%
  average execution versus `pir0-trace.jsonl`, Node24 hostless run-to-completion
  improved, and WebStandard retained-density RSS improved to 1,245,184
  bytes/runtime versus PIR5's 1,814,528 bytes/runtime. The WebStandard
  cooperative warm-pool path remains a measured latency exception: the original
  PIR0 Criterion baseline was 29.058-29.509 us, while the final focused REC5 run
  measured 890.72-908.86 us. REC0 through REC5 are now `done`, but this
  measured exception blocks using REC as performance justification for broader
  cooperative defaults or NodeFull cooperative admission until a targeted
  cooperative warm-pool overhead optimization closes.
- 2026-06-27 - Final REC archive closeout: closed the REC4 `nestedCalls`
  verifier gap with an explicit runtime/codegen Convex nested-call matrix and
  archived this plan. `bash scripts/verify-runtime-execution-classification.sh`
  passed with `Summary: 24 passed, 0 failed`. REC is now available as the
  canonical execution-plan/effect/authority vocabulary for NFR and future
  scheduler/pool/default work; the REC5 cooperative warm-pool latency exception
  remains a constraint on broader cooperative default promotion.
