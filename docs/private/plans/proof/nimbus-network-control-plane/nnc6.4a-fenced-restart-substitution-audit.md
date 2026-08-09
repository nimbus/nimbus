# NNC6.4a Fenced Restart Substitution Audit

Status: `R0-R4 full review complete; accepted corrections in progress`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

Audit base: `c09ee6015ecd9164b98fa4d1f84bb26214ddedde`

## Scope

NNC6.4a installs the first production tenant-workload restart authority.
NNC5.6 made sandbox inspection side-effect-free. NNC6.4 deleted the old
service `stop` then `start` restart path. This audit defines the portable
restart state, compute coordination, provider commands, service surface,
failure behavior, and deletion gates before a product edit.

The original audit and R0 checkpoints did not restart a workload, change a
provider, add a route, change the SDK, or alter the workload store. R1 added
the portable restart model and its exact durable store content. R2 added the
sole compute orchestration and real Container/Krun capability adapters.

R3
added the service, SDK, and forwarded-machine cutover. R4 completed the live
watch, caller convergence, scheduler deletion, and acceptance gates. The one
full item review then found accepted defects in lifecycle authority, recovery,
fencing, and proof strength. The candidate is no longer frozen while those
corrections are in progress.

## Audit Baseline Result

At the read-only audit checkpoint, Nimbus had no active production
tenant-workload restart loop. This was the safe baseline state:

- Container and Krun inspection report exact exit, restart, cleanup, and
  snapshot-version evidence without effects.
- Exited workloads keep their authority, project `Stopping`, and publish no
  endpoints.
- The workload saga has provision and teardown phases but no same-generation
  restart state.
- Compute can observe a `SandboxInspection`, but it has no restart reducer,
  command, capability, or driver.
- The service API and SDK expose `get`, `start`, and `stop`. The SDK self-test
  requires `restart` to remain absent until this item.
- Tenant-workload systemd requests use `Restart=No` and reject an external
  restart policy before provider effects.

NNC6.4a could therefore add one authority directly. It needs no compatibility
bridge and must not create a second scheduler in sandbox, services, server,
CLI, node, machine, or network code.

## Written Acceptance Contract

| ID | Verifiable success criterion |
| --- | --- |
| A1 | The current source census and call graphs name every production restart-policy, exit-observation, provider-reset, service-route, workload-store, machine-forwarding, node, and Compose authority in scope. |
| A2 | `nimbus-workloads` owns a closed portable restart policy, trigger, request, epoch, execution-attempt identity, phase, command claim, result, and durable history vocabulary. |
| A3 | One restart record is nested in the existing workload saga. It retains the same desired and network generation while every new execution and publication effect carries a new `WorkloadExecutionAttemptId`. |
| A4 | Compute is the only restart decision and transition writer. Inspection, GET, service naming, provider status, and system projection remain read-only. |
| A5 | The restart-admission CAS binds the exact saga, tenant-qualified source, desired generation and digest, current revision, trigger, inspection version when exit-driven, provider selection, restart epoch, policy-attempt count, and stable request ID. |
| A6 | Automatic policy restart and explicit service restart enter the same reducer. Explicit restart does not consume the automatic policy limit. Concurrent triggers admit at most one restart epoch. |
| A7 | The persisted order is observe exit or admit explicit request, commit restart, withdraw publication, quiesce and reset execution, wait for the persisted schedule, prepare the next attempt, reacquire the same-generation attachment and required PEP, activate, prove readiness, publish, and observe. |
| A8 | No restart effect occurs before the restart-admission CAS. Each later effect has its own exact claim CAS and result CAS. Ambiguity permits exact inspection only; only authenticated absence permits a higher dispatch epoch. |
| A9 | Withdrawal or a successor generation that wins before admission vetoes restart with zero effects. Withdrawal that wins later vetoes every unissued command and fences or inspects an issued ambiguous command. |
| A10 | Container and Krun implement the same small execution-attempt capabilities. Host-managed and machine-forwarded attachment and PEP effects remain in `nimbus-sandbox`; no effect enters `nimbus-network`. |
| A11 | Restart-retained detach never releases a network allocation, port lease, attachment identity, or PEP authority. Reattach authenticates the same network generation and the new execution attempt before tenant execution. |
| A12 | The durable absolute not-before value and completed automatic-policy count live in the workload saga. A clock rollback can delay restart but cannot authorize it early or reset the count. |
| A13 | Provider manifests keep only exact effect receipts needed for inspection and replay. `next_restart_at_millis` and dormant provider-local scheduling are deleted; any retained count is an observed mirror and cannot authorize restart. |
| A14 | The native service `POST .../restart` route and Nimbus SDK method require an exact source-generation precondition and a stable idempotency request ID, then submit the same compute restart transition. They never call stop then start. |
| A15 | The live automatic trigger is a bounded compute-owned restart watch over durable running/observed sagas. A provider inspection can propose evidence but cannot submit or execute a restart. GET and logical name resolution remain effect-free. |
| A16 | Tenant-workload node providers continue to require `Restart=No`. Forwarded-machine restart commands preserve the complete saga, generation, attempt, epoch, and inspection fences; no coarse guest restart route exists. |
| A17 | Cancellation before durable submission makes zero store or provider calls. Cancellation after submission cancels only the waiter; durable work remains recoverable. |
| A18 | Deterministic success, boundary, failure, ambiguity, crash, fresh-process, concurrency, withdrawal-race, stale-fence, policy-exhaustion, attachment/PEP, service API, SDK, node, machine, and Compose tests pass with exact counts. |
| A19 | NNCV034 and its mutation suite pass. Existing NNCV000-NNCV033, the `nimbus-network -> nimbus-core` edge, sovereignty tripwire, bind census, and side-effect-free inspection contract remain green. |
| A20 | All affected behavior, all-target check, strict Clippy, warning-denied rustdoc, format, diff, SDK, docs, and modularity gates pass. One Sol/xhigh/fast structured review runs only after A1-A20 are green and the item is candidate-frozen. |

## Audit Baseline Source Census

This census records the pre-implementation product source unless a row says
`test-only`. The R1-R2 checkpoint table below records the implemented delta.

| Concern | Current source-derived result |
| --- | --- |
| Desired restart policy | `SandboxLifecycleSpec` owns `Never`, `OnFailure`, and `Always`. Compose lowers its file policy into this spec. The portable saga cannot inspect the opaque executable to schedule a restart. |
| Read-only observation | `SandboxInspection` separates handle, execution, restart assessment, cleanup finality, and an opaque SHA-256 comparison version. |
| Container inspection | `backends/container/runtime/inspection.rs` reports an exited candidate from authenticated manifest and exit evidence. It does not reset, detach, launch, repair PEP, or write. |
| Krun inspection | `backends/krun/vm/inspection.rs` has the same read-only behavior and evidence shape. |
| Policy classifier | `backends/inspection.rs` is pure and clock-free. It reads policy, exit code, completed count, optional historical deadline, shutdown, and blocker evidence. Its result is not command authority. |
| Backend schedule fields | Both manifests retain `restart_count` and `next_restart_at_millis`. Production has no count increment. The Container decision helper that changes them is in a `#[cfg(test)]` restart module. |
| Container restart effects | `runtime/restart.rs` contains restart-retained runtime deletion, attachment detach, PEP stop, machine publication withdrawal, and receipt cleanup. The module is test-only today. The underlying Container relaunch path is production-compiled. |
| Krun restart effects | `vm/lifecycle.rs` contains restart-retained attachment and PEP primitives. Its `reset_runtime_for_restart` wrapper is test-only. |
| Shared lower network seam | OCI attachment lifecycle already distinguishes `FreshLaunch` from `RestartRetained`. Restart detach retains authority; terminal detach releases it. |
| Workload intent | `WorkloadSagaIntent` records desired state, generation, executable, source, network, activation, publication, and admission. It has no portable restart policy. |
| Workload state | `WorkloadSagaRecord` has active and successor intent, revision, phase, provision disposition, transition, and failure. There is no restart state. |
| Equal generation | Reapplying the exact intent is unchanged; divergent content conflicts. A higher generation begins withdrawal. No operation can express a new execution attempt for the same desired generation. |
| Execution identity | `WorkloadExecutionId` derives from workload UID, assigned node, and desired generation. A same-generation restart has no distinct attempt identity and is vulnerable to stale-callback ABA without a new fence. |
| Recovery | `WorkloadSagaAction` covers provision, teardown, cleanup inspection, successor promotion, and quiescence. It has no restart action. |
| Compute projection | `validate_execution_observation` consumes only the handle from the typed inspection. It does not admit or drive restart from the restart assessment or inspection version. |
| Service surface | `ServiceLifecycleVerb` is `Get`, `Start`, or `Stop`. The server router has `/start` and `/stop`, but no `/restart`. |
| SDK surface | The route table has service start and stop. The type self-test marks `services.restart` as an expected error until the fenced saga exists. The README already documents the intended method. |
| Runtime naming | `nimbus-services` binding snapshots and resolution are read-only. They have no restart capability and must stay that way. |
| Node provider | Host lifecycle plans use `Restart=No`; non-`No` values fail before provider inspection or reconciliation. |
| Forwarded machine | The internal Machine API carries complete `SandboxInspection` evidence and exact provision commands. It has no restart command or coarse service-restart endpoint. |
| Compose | Compose preserves the restart policy but rejects a terminal or stopping same-generation observation because restart is not provision. A later command can reopen Engine durability; Compose owns no local restart store. |

### Evidence locations

- `crates/nimbus-workloads/src/desired.rs:6`
- `crates/nimbus-workloads/src/saga.rs:353`
- `crates/nimbus-workloads/src/saga.rs:417`
- `crates/nimbus-workloads/src/saga.rs:944`
- `crates/nimbus-workloads/src/saga/state.rs:42`
- `crates/nimbus-workloads/src/saga/state.rs:589`
- `crates/nimbus-workloads/src/saga/state.rs:1174`
- `crates/nimbus-compute/src/workload_saga/recovery.rs:14`
- `crates/nimbus-compute/src/workload_projection.rs:589`
- `crates/nimbus-sandbox/src/inspection.rs:6`
- `crates/nimbus-sandbox/src/backends/inspection.rs:18`
- `crates/nimbus-sandbox/src/backends/container/runtime/inspection.rs:214`
- `crates/nimbus-sandbox/src/backends/container/runtime/restart.rs:22`
- `crates/nimbus-sandbox/src/backends/krun/vm/inspection.rs:152`
- `crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs:1210`
- `crates/nimbus-compute/src/services.rs:299`
- `crates/nimbus-server/src/router.rs:820`
- `packages/nimbus/src/selftest.mjs:731`
- `crates/nimbus-node/src/host_lifecycle.rs:252`
- `crates/nimbus-node/src/host_lifecycle.rs:527`
- `crates/nimbus-cli/src/compose/lifecycle.rs:221`

## Current And Target Call Graphs

### Current exit observation

```text
Container or Krun exits
  -> durable provider exit receipt remains
  -> SandboxBackend::inspect
     -> read existing lifecycle lock and manifest
     -> read exact runtime/exit evidence
     -> pure restart assessment
     -> SandboxInspection(version, Stopping, Retained)
  -> upper projection rejects Ready publication
  -> no restart effect
```

### Current explicit service surface

```text
authorized service route
  -> Get   -> read definition and observed projection
  -> Start -> compute provision saga
  -> Stop  -> later NNC6.5 retirement path

no Restart route, verb, SDK route, or compute transition exists
```

### Target automatic restart

```text
bounded compute restart watch
  -> load exact Running + Observed saga record
  -> read exact selected execution provider inspection
  -> validate desired source, portable policy, provider report, exit,
     inspection version, current revision, and no withdrawal/successor
  -> CAS WorkloadRestartRequest with restart epoch + attempt ID + schedule
  -> restart driver resumes only from durable state
```

### Target explicit service restart

```text
authorized POST /api/tenants/{tenant}/services/{service}/restart
  -> require source generation + stable request ID
  -> compute resolves exact durable saga revision
  -> admit explicit trigger through the same restart reducer
  -> return or wait on the same durable restart epoch
```

### Target provider choreography

```text
durable restart request
  -> withdraw current publication, retain owner authority
  -> quiesce/reset exact execution attempt
     -> runtime absent
     -> attachment detached with RestartRetained
     -> PEP and machine forwarding withdrawn but retained
  -> wait until persisted not-before
  -> prepare next WorkloadExecutionAttemptId
  -> reattach same NetworkResourceGeneration
  -> authenticate attachment + PEP readiness
  -> activate tenant execution
  -> inspect workload readiness
  -> publish
  -> observe
  -> record restart history and return to idle Observed state
```

Every arrow after admission is a separate claimed command and durable result.
An effect result never advances two arrows.

## Frozen Durable Model

### Portable intent

`WorkloadSagaIntent` gains a closed `WorkloadRestartPolicy`. Compute derives
it from the admitted sandbox spec. The desired digest covers it. Compute must
cross-check the portable policy with the decoded executable before submission.
`nimbus-workloads` must not depend on `nimbus-sandbox`.

The policy vocabulary is:

```text
Never
OnFailure { max_restarts }
Always { max_restarts }
```

### Restart identity and history

The existing `WorkloadExecutionId` remains the stable same-generation
execution owner. A new `WorkloadExecutionAttemptId` identifies one process
incarnation. Attempt zero is the initial provision. Each admitted restart gets
one monotonic `WorkloadRestartEpoch` and one derived attempt ID.

Execution and publication evidence must carry the attempt ID. The network
reference keeps the same workload/network generation. This separation avoids
using an IP address, port, PID, sandbox handle, or provider token as identity.

The saga stores:

- completed restart epoch.
- completed automatic-policy restart count.
- the active restart request, if present.
- last completed restart evidence.
- current execution-attempt ID.
- stable explicit request ID or exact exit-driven inspection version.
- persisted absolute not-before time.
- command claim, dispatch epoch, and result evidence for the active step.

An explicit restart increments the restart epoch but not the automatic-policy
count. An automatic restart increments both exactly once at admission.

### Nested restart state

Restart is a nested state machine in the existing workload saga record. It is
not a synthetic higher desired generation and it does not use the final
teardown graph.

The closed phases are:

```text
Idle
Requested
PublicationWithdrawalPending
ExecutionQuiescencePending
Scheduled
PreparationPending
AttachmentPending
ActivationPrerequisitePending
ActivationPending
ReadinessPending
PublicationPending
ObservationPending
```

Each pending effect uses a closed disposition:

```text
Ready
DispatchPending
InspectionRequired
DefiniteFailure
```

`requires_recovery` is true for every non-idle restart phase except a future
scheduled request whose persisted not-before value is not due. Recovery must
still expose the next due time so the worker does not busy-spin.

On success, the saga installs effect references for the new attempt ID. The
restart state returns to `Idle`. The top-level desired generation remains
`Observed` and unchanged.

### Clock contract

Compute injects a clock into the pure restart reducer and persists the
absolute not-before value in the admission transition. It never recomputes the
deadline from process start or provider manifest state. A backward wall-clock
step delays eligibility. It cannot authorize an early command. Tests use a
deterministic clock and prove process recreation does not reset the deadline
or count.

### Trigger contract

Automatic restart is driven by a bounded, paginated compute watch over durable
running and observed sagas whose portable policy is not `Never`. It is not
driven by GET, logical name resolution, a provider callback with command
authority, or a sandbox-local scheduler.

The watch can consume provider exit notification as a wake-up hint, but it
must load the durable saga and exact inspection before admission. Hints are not
authority. NNC6.1e2 still owns the final all-phase startup recovery scanner.

An explicit service restart uses a stable request ID and a required source
generation. Compute resolves and CASes the current internal saga revision.
The public API does not expose the internal revision as user authority.

## Small Capability Matrix

| Capability | Effect owner | Real substitutions | Exact result |
| --- | --- | --- | --- |
| `RestartPublicationWithdrawalCapability` | current ingress owner | sandbox direct/machine publication and server ingress where selected | current publication absent or exact in-progress/ambiguous evidence; lease authority retained |
| `WorkloadExecutionQuiescenceCapability` | execution provider | Container and Krun; forwarded adapters carry the same command | old attempt runtime absent, restart-retained detach complete, exit receipt retained until exact completion |
| `WorkloadRestartPreparationCapability` | execution provider | Container and Krun | new attempt receipt prepared; no tenant execution or host-routable publication |
| existing attachment capability with restart context | attachment provider | Container and Krun OCI attachment adapters | same network generation attached with new attempt fence and required PEP evidence |
| existing activation prerequisite, activation, readiness, publication, and observation capabilities with restart context | current named owners | their existing real providers | exact attempt-specific evidence |

There is no `RestartProvider`, `NetworkProvider`, or coarse
`SandboxBackend::restart`. The registry selects the exact admitted provider and
does not fall back to the first available implementation.

## Lifecycle And Race Matrix

| Current durable state or race | Allowed result | Forbidden result |
| --- | --- | --- |
| `Observed`, runtime present | No automatic restart | Policy or GET causes reset. |
| `Observed`, eligible exit, exact inspection | Admit one restart request by CAS. | Inspection itself launches or detaches. |
| Policy `Never` | Keep retained, unpublished evidence. | Restart request or effect. |
| `OnFailure`, exit code zero | Keep retained evidence. | Automatic restart. |
| Policy count exhausted | Keep retained evidence and exact count. | Counter reset after process restart. |
| Eligible exit below limit | Persist count, epoch, attempt, and schedule once. | Provider manifest becomes scheduler authority. |
| Explicit service request | Admit same reducer with explicit trigger. | Local stop/start or automatic-policy count increment. |
| Duplicate explicit request ID | Return the same admitted epoch/result. | Second restart epoch. |
| Two different concurrent triggers | One CAS winner. | Two provider effects for one step. |
| Withdrawal wins before admission | Restart conflict and zero effects. | Restart from stale inspection. |
| Restart wins, then withdrawal arrives | Fence every unissued restart command; inspect issued ambiguity. | Later activation or publication under withdrawn desire. |
| Stale generation, digest, revision, attempt, epoch, provider, or inspection version | Reject before effect and preserve bytes. | Best-effort retry or inferred adoption. |
| Crash after effect before result CAS | Inspect exact command and attempt. | Repeat the effect without exact absence. |
| Runtime reset succeeds, attachment detach is ambiguous | Retain authority and inspect. | Release capacity or activate. |
| Old runtime callback arrives after new attempt | Reject by attempt ID. | Mutate current status or endpoints. |
| Same-generation attachment and PEP ready | Permit activation of the exact new attempt. | Treat old-attempt readiness as current. |
| Readiness not exact | Wait or reject. | Publish. |
| Caller cancels before submission | Zero store/provider calls. | Background restart. |
| Caller cancels after submission | Waiter ends; durable work continues. | Rollback or delete the restart request. |
| Clock moves backward | Delay. | Early restart or recomputed deadline. |
| Fresh process | Reopen Engine record and inspect/resume. | Receive handed-over memory state or reset count. |

## Failure And Reconciliation Rules

1. A definite error leaves the last completed restart phase durable and emits
   no later command.
2. An ambiguous store CAS reloads the exact record. It never calls a provider
   until a fresh read confirms the transition.
3. An ambiguous provider effect changes the command disposition to inspection
   required. It never authorizes a new attempt ID or a direct retry.
4. Exact absence can authorize the same stable command at the next dispatch
   epoch. The semantic restart epoch and execution-attempt ID do not change.
5. Source, desired, provider-report, generation, or withdrawal changes fence
   new effects. Inspection remains available for already-issued ambiguity.
6. Restart-retained cleanup cannot release capacity. Unknown cleanup remains
   fenced for NNC3.8/NNC8.3 convergence.
7. A restart-specific withdrawal removes routability but retains lease and
   listener ownership. NNC6.5 owns final withdrawal and release.
8. The provider clears the old exit receipt only after it proves runtime
   absence and restart-retained attachment disposition. It must also prove
   PEP or forwarding withdrawal and authenticate the new command result.
9. Observed provider counts are comparison receipts. Only saga history can
   admit or exhaust an automatic restart.
10. Operator-visible status distinguishes scheduled, restarting, blocked,
    failed, and cleanup-pending state without exposing provider handles or
    policy secrets.

## Fail-Before Test Packet

NNCV034 is the first executable change after this audit checkpoint. The audit
base must fail its live contract. Its mutation suite must pass. It extends the
existing source scanner and aggregate verifier rather
than adding a second Rust parser.

The live contract checks these groups:

1. portable restart policy, trigger, epoch, request, attempt ID, phase,
   command, result, and strict serialization.
2. one nested saga state, exact transition-ID coverage, `requires_recovery`,
   and no same-generation ABA.
3. pure eligibility, persisted schedule, automatic versus explicit counts,
   withdrawal veto, and deterministic clock.
4. confirmed command construction, exact fences, claim-before-effect,
   ambiguity inspection, exact-absence retry, and definite-error stop.
5. small capability registry with Container and Krun substitutions and no god
   provider.
6. same-generation restart-retained attachment and PEP readiness before
   activation.
7. explicit service route, SDK route/method, generation precondition, stable
   request ID, and no stop/start composition.
8. bounded automatic watch with effect-free GET and logical resolution.
9. forwarded-machine fence preservation and `Restart=No` node enforcement.
10. delete provider-local scheduler fields, dormant decision helpers, coarse
    restart routes, and network-crate effects.
11. required behavior, crash, process, race, cancellation, and SDK tests.
12. exact plan/proof/ledger completion tokens.

Mutation cases cover these defects:

- remove or cross each identity dimension.
- make a constructor forgeable.
- add an unknown enum variant.
- bypass admission CAS.
- retry ambiguity directly.
- reset the count or deadline.
- let withdrawal lose.
- activate before attachment and PEP readiness.
- accept an old-attempt callback.
- add a god provider or network effect.
- add local stop/start.
- remove API idempotency.
- enable node restart.
- discard a machine fence.
- restore a backend-local scheduler.

Each mutation must produce one named NNCV034 diagnostic.

The initial expected-red baseline is:

```text
NNCV000-NNCV033: green
NNCV034: red only because the frozen restart contract is not implemented
NNCV034 mutation suite: green
```

No product edit starts until the owner commits the helper, aggregate
integration, exact summary count, and expected-red proof.

### R0 expected-red evidence

- `scripts/verify-nimbus-network-source-contract.mjs` extends its existing
  mode router with `workload-restart-contract`. It uses the same extracted
  lexical scanner as every retained source mode. No second Rust parser exists.
- The extraction leaves the original mode router at `1,749` lines. The shared
  lexical module is `227` lines, and the restart-specific checker is `772`
  lines. Each file remains below the repository decomposition threshold.
- NNCV034 has `19` contract groups. Its fixture proves the frozen green target.
  Its `33/33` mutations each change the fixture and produce one exact named
  diagnostic.
- The live aggregate has the expected-red result: `34` passed and `1` failed.
  NNCV034 is
  the only failure. Its diagnostics name the absent state, command, provider,
  route, SDK, watch, machine, and behavior contracts.
- The aggregate mutation suite passes `360/360`, including all retained
  NNCV000-NNCV033 cases and the `33` additive NNCV034 cases.
- `git diff` from audit checkpoint
  `8723bc9a8ac27abc8ecbbd59d5f8d8d159e98cc1` contains only the R0 verifier,
  proof, plan, and routing paths. Product source remains byte-identical.

## Behavioral Proof Matrix

| Proof family | Required cases |
| --- | --- |
| Pure reducer | `Never`; `OnFailure` clean/failing; `Always`; zero, below-limit, and exhausted count; explicit trigger; duplicate request; deadline not due/due; backward clock; blocker; withdrawal and successor veto. |
| Portable wire | Exact round-trip; missing, unknown, duplicate, crossed, digest-divergent, stale revision, stale generation, stale epoch, stale attempt, and stale inspection rejection. |
| Command protocol | Initial claim; one CAS winner; exact replay; definite failure; ambiguity; in-progress; exact absence; higher dispatch epoch with stable semantic IDs; result crossing. |
| Container | Natural exit, running explicit restart, host-managed PEP, machine forwarding, partial reset, stale receipt, byte stability on fence failure, reattach, activate, and old callback rejection. |
| Krun | Same matrix as Container with host-managed attachment and VMM absence. |
| Attachment and PEP | Same network generation, restart-retained authority, no release/reuse, exact PEP generation, missing/stale PEP, and no activation before complete readiness. |
| Service API | Authorization, cross-tenant rejection, missing/stale generation, duplicate request ID, concurrent request IDs, wait behavior, response status, and no stop/start calls. |
| SDK | Route parity, types, generated request ID reuse for one call, wait conditions, browser package closure, README example, and removal of the expected-error marker. |
| Automatic watch | Bounded page, no busy-spin before due time, exact provider selection, read-only hint, process restart, and no effect from GET/name resolution. |
| Withdrawal races | Withdrawal before inspection, before admission CAS, after admission, before each effect claim, after effect before result CAS, and during provider ambiguity. |
| Cancellation | Before insertion, after insertion, during wait, and after provider ambiguity. |
| Fresh process | Reopen Engine only at every restart phase and choose wait, inspect, dispatch, fence, or complete without handed-over memory. |
| Forwarded machine | Exact wire round-trip, crossed provider/attempt/epoch/version rejection, response-loss inspection, and no coarse restart route. |
| Node | DirectProcess and Systemd keep `Restart=No`; `OnFailure` and `Always` reject before effects. |
| Compose | Local and forwarded eligible restart use Engine and compute; a fresh command resumes; Quadlet/Kubernetes export remains explicitly external-provider-managed output. |
| Projection | Scheduled/restarting/blocked/failure status is truthful; old attempt endpoints never project; naming remains logical and read-only. |

## Implementation Bands Within NNC6.4a

These bands are development order, not review units or completion units.
NNC6.4a remains one coherent item because no production restart authority
exists and the complete value is one end-to-end restart lifecycle.

### Band R0: expected-red static contract

- Add the NNCV034 helper, mutations, aggregate integration, and proof result.
- Freeze the exact product-path allowlist from the source scanner.
- Product source remains unchanged.

### Band R1: portable state and pure decisions

- Add portable policy and restart vocabulary to `nimbus-workloads`.
- Add attempt IDs to execution/publication evidence and transition digests.
- Add nested restart state, history, legal transitions, recovery decisions,
  and strict wire tests.
- Extend the server store codec/schema only for exact durable content and
  bounded watch queries. No provider effect.

### Band R2: confirmed commands and provider adapters

- Add compute reducer, command confirmation, dispatcher, registry, driver,
  deterministic clock, and restart watch.
- Add record-owned claim, inspection, exact-absence retry, and correlated-result
  transitions. Compute must not fabricate a portable claim whose constructor
  is workloads-private.
- Add one bounded, global, clock-free restart-candidate store query. It returns
  every active restart. It also returns inactive observed/running candidates
  with a non-`Never` policy. The compute watch applies due-time policy with its
  injected clock.
- Generalize the one sandbox provider-command journal with a monotonic restart
  ordinal. Do not create a second journal or reuse workload/network generation
  as the ordinal.
- Promote restart-specific capability traits only after Container and Krun
  both implement them.
- Use current owner-local ingress, attachment, PEP, runtime, and machine
  effects. Do not add a coarse backend method.

### Band R3: caller cutover and deletion

- Add the service route and SDK method through the compute restart submission
  seam.
- Route local and forwarded Compose reconciliation through the same seam.
- Delete dormant local scheduling, obsolete backend deadline fields, SDK
  expected-error marker, and any remaining local stop/start restart path.
- Keep final stop, force delete, Compose down, and retirement in NNC6.5.

### Band R4: acceptance convergence

- Run all behavior, race, crash, process, SDK, static, quality, docs, and
  modularity gates.
- Freeze one complete candidate.
- Run exactly one Sol/xhigh/fast structured item review.
- If an accepted executable defect changes code, rerun affected proofs and
  one narrow correction review only.

## Candidate Product-Path Allowlist

R0 may edit only:

- `scripts/nimbus-network-control-plane/workload-restart-contract.sh`.
- its concept-owned self-test if separate.
- `scripts/verify-nimbus-network-source-contract.mjs` and a concept-owned
  scanner child or shared lexical module. The child keeps the existing scanner
  below the repository decomposition threshold. The extension uses one source
  scanner. It must not add a second Rust parser.
- `scripts/verify-nimbus-network-control-plane.sh`.
- this proof.
- the canonical plan.
- `docs/private/plans/README.md`.

R1-R4 may edit concept-owned files under these exact roots when the diff maps
to A1-A20:

- `crates/nimbus-workloads/src/saga.rs`.
- `crates/nimbus-workloads/src/saga/restart.rs` and its tests.
- `crates/nimbus-workloads/src/saga/state.rs` and restart state child/tests.
- `crates/nimbus-workloads/src/store.rs` and store conformance tests.
- `crates/nimbus-compute/src/workload_saga.rs` and concept-owned restart
  reducer, confirmation, dispatcher, driver, provider, watch, and tests.
- `crates/nimbus-compute/src/workload_projection.rs` and tests.
- `crates/nimbus-compute/src/state.rs` only to retain the one complete restart
  registry, driver, supervisor, and bounded watch in the existing managed
  workload composition. It gains no provider effect or second coordinator.
- `crates/nimbus-compute/src/workload_provisioner.rs` and tests only where R2
  generalizes the shared cancellation and tracked-work seam.
- `crates/nimbus-compute/src/services.rs` and tests.
- `crates/nimbus-services/src/catalog.rs`, exact sandbox projection and
  retirement paths under `src/manager`, and their concept-owned tests only
  for attempt-fenced read models. These paths keep logical names and
  projections read-only. They gain no restart decision, schedule, or provider
  effect.
- `crates/nimbus-server/src/workload_saga_store.rs`, its codec/schema/recovery
  children, and exact process tests.
- `crates/nimbus-server/src/http/services.rs`, `router.rs`,
  `workload_composition.rs`, `state.rs`, and focused tests.
- `crates/nimbus-server/src/listener_lease.rs` and its restart-retention child
  only for stop-and-join plus exact lease retention before same-port rebind.
- `crates/nimbus-server/src/workload_ingress.rs` and its concept-owned tests.
  These paths own only source-attempt withdrawal, retained listener ownership,
  target-attempt publication, and read-only observation through the existing
  ingress owner.
- `crates/nimbus-sandbox/src/inspection.rs` and the pure classifier/tests.
- `crates/nimbus-sandbox/src/lib.rs`, `execution_attempt.rs`,
  `provider_command.rs`, and `provision.rs` plus their concept-owned tests.
  Exact provision integration callers can add the provider-neutral attempt
  fence and generalize the one existing provider-command journal. These paths
  cannot add restart policy, scheduling, or a second journal.
- Container and Krun manifest, inspection, restart, provision, attachment,
  readiness, PEP, machine-forwarding, state-summary, and concept-owned tests.
- `crates/nimbus-machine/src/api.rs` only if the existing generic workload
  command or inspection wire gains the restart fences.
- exact `crates/nimbus-cli/src/machine/{api,backend,client,stub}` workload
  command adapters and tests.
- `crates/nimbus-node/src/host_lifecycle.rs`, provider adapters, reconciler,
  and tests only for attempt propagation and the retained `Restart=No` proof.
- `crates/nimbus-cli/src/compose` lifecycle/composition paths and tests.
- `packages/nimbus/src/control-plane/client.ts`, route table, public types,
  self-test, package tests, and `packages/nimbus/README.md`.
- `crates/nimbus-system/src/inventory.rs` and service status projections.
- `crates/nimbus-network/src/port_lease/lifetime.rs` and the rebind owner/tests
  only for an atomic authenticated subset transition from confirmed-stopped
  listeners to restart-retained leases. This is portable lease state, not a
  socket or restart effect. Unrelated plan members remain unchanged.
- the NNCV034 helper, aggregate verifier, this proof, plan, and routing index.
- The workload network-plan durability contract script, for v4 assertions
  only.

This amendment aligns its strict wire and transition-domain assertions with
the v4 durable record. It does not change NNC6.2a ownership or behavior. It
also does not change the frozen source diff or completion checkpoint.
- `crates/nimbus-workloads/src/saga/provision.rs` only for an explicit
  concept-owned `large_enum_variant` reason on the existing closed provision
  result. The execution-attempt fence makes its success evidence cross
  Clippy's size heuristic. The pure reducer and strict portable wire keep the
  value inline. Allocation would touch all provision producers and broaden R1
  without improving restart behavior.
- `crates/nimbus-compute/src/workload_saga/recovery.rs` only for the same
  explicit size-heuristic reason on the existing low-rate pure recovery
  action. The larger execution reference crosses the heuristic. Its complete
  provider-neutral evidence remains inline. R1 adds no R2 coordinator or
  effect.

Any path outside this allowlist requires a recorded proof amendment before the
edit. Generated, vendored, unrelated adapter, cluster, and storage-data-plane
paths are forbidden.

## Explicit Non-Goals And Retained Owners

- NNC6.5 owns general withdrawal, drain, stop, detach, release, record,
  force-delete, Compose-down, failed-provision compensation, and retirement
  caller cutover. NNC6.4a implements only restart-specific retained
  withdrawal and quiescence.
- NNC6.6 owns logical resolver fencing during withdrawal.
- NNC6.1e2 owns final all-phase fresh-process startup recovery and tenant
  retirement. NNC6.4a proves restart-specific record reopen and resume.
- NNC3.8 and NNC8.3 own ambiguous cleanup convergence, finalization, orphan
  removal or quarantine, release, and capacity reuse.
- NNC8.4 owns the broad stale-generation and stale-callback campaign.
  NNC6.4a still proves every direct restart fence.
- `nimbus-services` keeps logical names, definitions, sessions, and read-only
  snapshots. It gets no restart scheduler or provider effect.
- `nimbus-tenant` keeps admission and quota policy.
- `nimbus-egress` remains PDP. `nimbus-proxy` and sandbox PEP adapters remain
  enforcement and forwarding owners.
- `nimbus-network` keeps portable connectivity identities and leases. It gets
  no socket, process, policy, scheduler, Netavark, nftables, gvproxy, or
  restart effect.
- Machine lifecycle restart and the Nimbus daemon's service-manager restart
  are not tenant-workload restart.
- Quadlet and Kubernetes export are external-provider-managed output. Their
  restart policy does not become Nimbus runtime authority.
- Cluster membership, routing, Iroh, openraft, and overlay transport remain
  out of scope.

## Seam Checklist

| Seam | Closeout proof |
| --- | --- |
| One desired policy | Portable intent matches the admitted executable; no second config source. |
| One durable restart history | Workload saga count, epoch, deadline, and request are canonical; provider fields are receipts only. |
| One coordinator | Only compute commits restart transitions and issues commands. |
| Query/command split | Inspection, GET, naming, and projection make zero lifecycle effects. |
| Stable identity | Tenant-qualified saga plus desired generation plus restart epoch plus execution-attempt ID; no IP, port, PID, or handle identity. |
| Desired/durable/observed split | Intent and policy, saga restart record, and provider/system observations are different values and stores. |
| Exact selection | Every command uses the admitted provider and fails closed without it. |
| Small capabilities | Container and Krun earn execution seams; current ingress/attachment owners keep effects. |
| PDP/PEP | Restart checks current PEP readiness but does not evaluate or forward policy. |
| Same-generation network | Attachment and leases remain generation-stable; attempt-specific execution evidence changes. |
| No early routability | Withdrawal precedes reset; publication follows exact new-attempt readiness. |
| Ambiguity | Inspect exact attempt before retry; exact absence only; no inference. |
| Withdrawal priority | Durable withdrawal or successor fences new restart effects. |
| No cycle | `nimbus-network -> nimbus-core` remains the only network workspace edge. |
| No dual authority | No local scheduler, stop/start shim, provider auto-restart, or coarse guest route remains. |
| Later-owner integrity | NNC6.5, NNC6.6, NNC6.1e2, NNC3.8, NNC8.3, and NNC8.4 obligations are linked, not implemented or duplicated. |

## Audit Evidence And Next Checkpoint

| Checkpoint | Evidence |
| --- | --- |
| Worktree integrity | Owner worktree was clean at `c09ee6015ecd9164b98fa4d1f84bb26214ddedde` before this proof. Read-only audit agents changed zero paths. The original checkout remained untouched. |
| Source audit | Direct source inspection covered workloads intent/state/recovery/store, compute provision/projection/services, Container/Krun inspection and restart-retained primitives, services naming/retirement, server routes/store, SDK, Compose, Machine API, node DirectProcess/Systemd, and overlapping plan owners. |
| Architecture decision | Use one nested same-generation restart state, separate execution-attempt identity, compute-owned durable schedule/count, a bounded compute watch, exact retained provider commands, and one explicit service ingress. Do not use a higher desired generation, local stop/start, or a god provider. |
| Current green baseline | NNC6.4 item commit `6f4f909a06a20de1003d5aafc2f5ffcba43cf0bd`: NNCV033 `40/40` and `50/50`, aggregate `34/34` and `327/327`, affected behavior, SDK, docs, quality, static, and modularity gates green. |
| Audit checkpoint verification | On the audit candidate, the live static verifier passes `34/34`, its fail-closed mutation self-test passes `327/327`, docs pass `108`, the site gate passes `17/17`, and this proof passes technical-writing lint with zero errors. |
| R0 fail-before checkpoint | NNCV034 owns `19` contract groups and `33/33` exact named mutations. The live aggregate has only the intended NNCV034 failure at `34` passed and `1` failed. The aggregate mutation suite passes `360/360`. Product source is unchanged. |
| R1 portable-state candidate | Workloads format v4 owns the closed restart policy, trigger, admission, request, epoch, dispatch, attempt, phase, claim, result, disposition, full completed admission history, and recovery-decision vocabulary. The nested state retains one desired and network generation while execution and publication evidence bind the new attempt. Pure restart behavior passes `17/17`; exact server-store behavior passes `9/9`. |
| R1 persistence and recovery boundary | The Engine-backed store requires `restartPolicy` and `restartState`, rejects missing, null, unknown, duplicate, crossed-identity, crossed-digest, crossed-epoch, and crossed-attempt content, preserves the absolute deadline and policy count across Engine reopen, admits one epoch under CAS contention, and returns bounded stable completed pages. These are Engine-reopen proofs, not distinct-process proofs; R4 retains the required distinct-process restart case. |
| R1 static checkpoint | Nine NNCV034 groups are green: vocabulary, nested state, admission identity, schedule, withdrawal, node, network, paths, and ledger. The ten planned R2-R4 groups remain red: reducer, command, ambiguity, readiness, capabilities, service, watch, machine, scheduler, and behavior. NNCV034 mutations pass `39/39`; the live aggregate has the required intermediate shape at `34` passed and `1` expected NNCV034 failure. The complete aggregate mutation suite passes `366/366` with zero `SELFTEST FAIL` lines. |
| R1 compatibility and quality | The retained NNC6.2a contract passes `24` direct checks and `10/10` mutations after its strict format assertion advances from v3 to v4. Full workloads behavior passes `155/155`; server workload-store behavior passes `48/48` with `5` declared child-only ignores; affected all-target check, strict Clippy, warning-denied rustdoc, Rustfmt, Prettier, diff, Bash syntax, and scoped ShellCheck pass. Proof lint has zero diagnostics. Docs pass `108`; the site gate passes `17/17`. Known vendored Brotli warnings are unchanged. |
| R1 scheduling semantics | A future durable deadline remains discoverable because the active record still requires recovery, but the pure recovery decision returns `WaitingUntil`; this prevents early effects and busy retry. Automatic request identity is rederived from saga and inspection evidence. Explicit restarts do not consume the automatic-policy count. |
| R1 strict-change posture | Nimbus is pre-launch. Format v4 is a direct strict replacement, with no v3 migration or compatibility shim. The temporary `WorkloadSagaIntent::new` default of `Never` exists only until R3 caller cutover and must be deleted or renamed before NNC6.4a completes. |
| R1 modularity | `saga.rs` (`1,618` lines) and `saga/state.rs` (`1,627` lines) are concept-owned composition roots in the explicit 1,500-1,999-line reason band. Restart invariants and transitions live in the `restart` children (`657`, `20`, and `555` lines); store behavior lives in its `485`-line concept-owned test child. No handwritten R1 file reaches 2,000 lines. |
| R1 durable checkpoint | Commit `d117ba369eaf5acc5ede9ec3edad32a11ddfbeb2`; staged tree `17f152b102a8ddf66d38d09535dc012161d592f1`; full patch SHA-256 `197410339314326a6ffe2827c14c12ef7b0a0ef6fa8fa9ed78c384837cbf547a`; executable and script patch SHA-256 `19ead57e9d0148691161645c6b2ef7886d682f5d44b91cb0351d8c6a4ab8ddeb`; `27` owned paths and zero unstaged paths. |
| R2 substitution audit | The provision confirmation pattern is reusable. The audit found and the portable/store checkpoint closed two gaps: record-owned claim/result transitions and a bounded global query for inactive automatic candidates. Container has dormant retained-reset mechanics; Krun's corresponding helpers are test-only. Both older launch helpers can publish before new-attempt readiness and cannot become the command adapter. The one provider journal must gain a same-generation restart ordinal because its current stream rejects a second attempt at the same workload generation. |
| R2 capability boundary | Compute owns one normalized automatic/explicit reducer, confirmation, driver, bounded watch, and exact registry. Workloads owns claim/result transitions and the clock-free candidate page. Container and Krun earn small withdrawal, quiescence, preparation/attachment, activation, and readiness capabilities. No `SandboxBackend::restart`, god provider, service route, SDK method, machine transport, local scheduler deletion, or `nimbus-network` change enters R2. |
| R2 fail-before checkpoint | The NNCV034 fixture now has `68/68` sole-diagnostic mutations. Tests are checked in their declared concept-owned files; the confirmed command is checked only in its exact module; R1 is frozen from `6d8961bd6d4da819b2524128cb398e22e0a9382f` through `d117ba369eaf5acc5ede9ec3edad32a11ddfbeb2`; and R2 is separately scoped from the R1 completion commit. The live contract exits `1` on exactly reducer, command, ambiguity, readiness, capabilities, service, watch, machine, scheduler, and behavior. Reducer, command, ambiguity, readiness, capabilities, and watch are R2 targets. Service, machine, scheduler, and behavior remain required R3/R4 red. |
| R2 portable command state | Workloads owns private initial claim construction, complete command-ID content, durable inspection, exact authenticated-absence retry on the same attempt at one higher dispatch epoch, correlated success receipts, and terminal failure. Standalone strict decoders reject crossed claims, forged receipts, and invalid definite-failure content. Command-state behavior passes `23/23`; full workloads passes `165/165`. |
| R2 global candidate store | `requires_restart_watch` is clock-free: every active restart is eligible, including explicit work under policy `Never`; an inactive record is eligible only at observed/running with a non-`Never` policy and no successor or failure. The required base-store method returns strict bounded pages ordered by stable saga ID. The Engine adapter owns one derived `restartWatchCandidate` field and one ordered index, and its decoder rejects a crossed physical mirror. Portable page tests pass `4/4`; durable Engine query tests pass `4/4` across tenants, pagination, Engine reopen, active/inactive cases, and insertion behind a cursor with next-sweep recovery. |
| R2 portable/store quality | The product checkpoint is `14e6236d4e3d1199a7ae40674bcdedd50b98fd58`; tree `1b36f0de809d52b3b07ed42bce0686af24e7b50b`; patch SHA-256 `2287a498552e12b4b15d828e95b29bdc85bbcc6b08e9b800d5a5be7499d22b08`; `33` paths. Server workload-store behavior passes `52` with `5` declared child-only ignores. Workloads, compute, server, and CLI pass all-target checks and strict Clippy; workloads/server pass warning-denied rustdoc; Rustfmt, Prettier, JavaScript syntax, and diff checks pass. Known vendored Brotli warnings are unchanged. |
| R2 compute admission | Checkpoint `8935e0c77dd188f50566b72c917b2005a213ecdd` adds one normalized automatic/explicit reducer and sole-coordinator CAS. Its tree is `324e36e979a54645c280edf9f4b136f1c9ed21fa`; its patch from the prior ledger checkpoint is SHA-256 `11b8bf2cb4fd94d79ce84f814c1fffd7917f10d92948407bc912a1b5a7ebeeb1` across `5` paths. The logic landed at `51653a091e457ae1950a91425fdea764829bae11`; the final checkpoint adds only canonical Rust formatting. Exact source revision, source generation, workload generation, desired digest, inspection version, provider selection, saga, request, withdrawal, and successor fences fail before CAS. Exact contention admits one epoch and adopts the idempotent loser. Cancellation before submission makes zero store calls. Reducer/progress behavior passes `8/8`; full compute passes `241` with one declared child-only ignore; strict compute Clippy passes. |
| R2 confirmed command and result | Checkpoint `e1e9c95167972c2566a468d01aa0b91e559dd9be`; tree `48f14195cfebd71e6ecaca0e012d83d6deefac56`; patch from the prior ledger checkpoint SHA-256 `3d5ed0766c5d4dcbbace69b4a1c69cb16662de499ad3d9f881accd7dc6238853`; `5` paths. One private constructor accepts only exact coordinator confirmation. The command retains stable key/saga/transition, desired/source, source and target attempts, restart and dispatch epochs, request, revisions, optional exit inspection, provider, step, claim, executable, and compiled network plan. Only the direct claim-CAS winner receives execute mode. Replay, confirmed ambiguity, and fresh-process recovery persist `InspectionRequired` before provider reads. Results authenticate durable transition, attempt, source, provider, claim, and epoch. Ambiguous or in-progress inspection never retries; authenticated absence retains the same attempt and advances exactly one dispatch epoch; definite failure is terminal. Focused behavior passes `10/10`; full compute passes `251` with one declared child-only ignore; strict compute Clippy passes. |
| R2 provider journal | One generalized sandbox provider-command journal replaces the provision-only owner. Its key authenticates source attempt when present, target attempt, workload generation, restart ordinal, provider realm, command operation, and exact durable observation. Provision uses ordinal zero; same-generation restart advances monotonically. Ambiguous effects retain inspect-before-retry behavior, and exact live absence is the only retry authority. |
| R2 compute orchestration | Compute owns one exact provider registry with no first-available fallback, one dispatcher, one phase-explicit driver, one deterministic clock, one retained supervisor, and one durable watch. Each sweep is bounded to `64` pages and retains its cursor; cancellation drops only waiters, not durable submitted work. Driver tests prove withdrawal before quiescence, retained detach before reattach, activation prerequisites before activation, readiness before publication, crossed callback rejection, ambiguity, exact absence, cancellation, and recovery. |
| R2 real provider substitution | Container and Krun implement the same small restart capabilities. Both authenticate exact source/target attempts and retained network generation before effects. Restart-retained detach preserves network allocation, lease, attachment, and PEP authority. Server ingress owns publication withdrawal/republish/observation and a concept-owned retained-listener authority. `nimbus-network` gains only attempt-fenced port-lease rebind and no provider effect or new workspace edge. |
| R2 provider capability truth | `ServerWorkloadProviders` requires every provision role and registers restart separately through `with_restart_capabilities`. Local Krun opts in after its real providers earn every role. A forwarded-machine realm remains unregistered until R3 adds its exact transport; it therefore fails restart dispatch closed instead of advertising a false capability. No compatibility shim or god provider is present. |
| R2 exact projection | `SandboxInspection::provider_authenticated_running` carries the exact execution attempt and provider-evidence digest. Service and standalone sandbox projections store the complete `WorkloadExecutionReference`, reject crossed owner/attempt/epoch evidence, and use small projection input records instead of parameter bags. Logical service naming remains read-only and services-owned. |
| R2 behavior | Focused restart behavior passes compute `55/55`, sandbox `69/69`, and server `20/20`. Full compute passes `289` with one declared child-only ignore; full sandbox passes `1,005` with `27` declared ignores; full server passes `594` with `31` declared ignores; full services passes `82/82`. Final post-adaptation evidence passes server composition `10/10`, CLI Compose lifecycle `6/6`, and guest provision `3/3`. |
| R2 quality | Affected all-target check passes across network, workloads, sandbox, services, compute, server, and CLI. Strict affected Clippy passes after correcting four exact local findings: redundant tuple parentheses, two eight-argument projection parameter bags, one `get(...).is_none()` assertion, and one clone of a copy digest. Warning-denied rustdoc passes for the six public affected crates. Rustfmt, diff, Bash syntax, and JavaScript syntax pass. Known vendored Brotli warnings are unchanged. |
| R2 integration failure dispositions | The first full Sandbox run found one direct cleanup-enum mismatch and two stale Krun error strings; all were corrected and the full suite passed. The first full Server run found `13` managed-fixture 500 responses because fake inspection evidence had no exact attempt; the shared fixture was corrected and focused plus full suites passed. CLI all-target compilation then exposed honest capability-reporting, provider-claim, and exact projection fixture drift; restart registration became explicit, provision claims use ordinal zero, and exact fixture fields were added. The first CLI guest test exposed two more strict execution/publication fixture fields; both were added and `3/3` passed. These were direct R2 convergence defects, not review findings. |
| R2 durable checkpoint | Product commit `5826dff6019d453f9eba575ecf67850ac3b19e6a`; tree `0a78ba83822755b9a0c730eee19fa0de5c077305`; patch from the prior ledger commit SHA-256 `59b5ee9e59b40f979d6cdbc0f0d832f5a3d9d5307f937409e293b48b36a37ae9`; `93` product/script paths. |
| R2 verifier state | NNCV034 passes `71/71` sole-diagnostic mutations. Its live contract exits `1` on exactly service, machine, scheduler, and behavior. Readiness, capabilities, watch, and paths are green. No final aggregate, docs, R4, or item-completion claim is made at this partial checkpoint. |
| R2 modularity | `workload-restart-source-contract.mjs` is `1,968` lines in the explicit-reason band. It remains the one deep NNCV034 owner for the production scan, green fixture, and sole-diagnostic mutations. R3 must extract fixture data before any other verifier growth reaches `2,000` lines and must reuse the existing parser. Lifecycle implementations and tests use concept-owned children; the generalized provider journal owns one coherent command-idempotency state machine. |
| R3 verifier extraction | Commit `e1c57ce9eba823b8dc7d0a5a4ab02d58fe4c0030`; tree `525ab0c5cc43c29dd2986ed9fd320e53cbb50d75`; patch from the R2 recovery commit SHA-256 `6d97796a59f4b3bc0ffe29797c47d93feab70d3654f1a774952f27cffa805753`; `2` script paths. The production contract is `1,527` lines and owns scanning plus mutation decisions. Its `471`-line sibling owns only green fixture construction and imports the existing scanner; no second parser exists. R2 is frozen through recovery commit `73f53796392eae1b7c6df06e15450f272e228710`, and R3 has a separate exact path scope. |
| R3 verifier extraction proof | JavaScript syntax, Prettier, and diff checks pass. NNCV034 remains `71/71` sole-diagnostic mutations. The live contract exits `1` with exactly service, machine, scheduler, and behavior; path scope is green. This is a partial R3 checkpoint, not an item-completion or review gate. |
| R3 completed-request replay | `WorkloadSagaRecord::admit_restart` recognizes the exact last completed request before revision admission, returns the existing epoch for exact content, and rejects crossed trigger, inspection, or schedule content. Focused workloads proof passes `2/2`. |
| R3 explicit compute submission | One generic `ExplicitWorkloadRestartSubmitter` validates the exact source identity and source generation, admits through the sole coordinator CAS, returns the active or completed durable receipt, and synchronously hands active work to the existing retained supervisor. Stable immediate submission uses durable not-before zero rather than wall-clock content. Duplicate submission, pre-submit cancellation with zero store/provider calls, and crossed source generation pass `3/3`. |
| R3 service and SDK cutover | The native service facade validates the declared service generation and uses the exact `SandboxBackedService` source identity. The authorized POST route returns honest `202 Accepted` admission evidence and never composes stop/start. The Nimbus SDK exposes the matching route, request, and response. Focused server proofs pass `2/2`: duplicate submission returns the same epoch and internal request ID, provider convergence advances the real start count, zero coarse stop calls occur, and crossed generation fails before admission. Both restart fixtures await `ServerFixture::shutdown`, so the restart runtime, router state, and process authority are released before their temporary roots disappear. Nimbus package build/test/typecheck pass with `24`-route parity. |
| R3 affected behavior and quality | Full workloads pass `167/167`; full compute passes `292` with one declared child-only ignore; and the Server library passes `596` with `31` declared ignores under the required `--test-threads=1` boundary for its deliberate process-global network authority. A diagnostic default-parallel run passed `557`, failed `39`, and ignored `31`; every failure was `DuplicateProcessComposition` between independent temporary roots, not a product assertion or restart behavior failure. Strict affected all-target Clippy, Rustfmt, Prettier, diff, docs `108`, and site `17/17` pass. Existing vendored Brotli warnings are unchanged. |
| R3 service/SDK durable checkpoint | Commit `99930529633e02f027ae17adaa8d7379a73af37b`; tree `af4a97170092e2b145b622b78bf62ddc544c4a9e`; patch from the prior recovery commit SHA-256 `587962a6ce7d8d146cc3a3b94fe2c4292c44c5a0a8c65a366198945d54035c2c`; `25` owned paths. This is a partial R3 recovery checkpoint, not an item-completion or review gate. |
| R3 service contract checkpoint | NNCV034 self-test remains `71/71`. Its live contract now exits `1` on exactly machine, scheduler, and behavior; the service and path groups are green. This is partial R3 progress, not item completion or candidate freeze. |
| R4 strict restart-policy proof | The fail-before Compose policy test passed `0/1`: `OnFailure { max_restarts: 3 }` composed as `Never`. `WorkloadProvisionSourceSnapshot` now preserves the admitted executable policy. The fixed proof passes `1/1`. The ambiguous `WorkloadSagaIntent::new` shortcut is now `new_without_automatic_restart`, and no production caller can silently choose `Never`. |
| R4 focused behavior | Parent forwarded adapter `4/4`, guest route `1/1`, Compose restart `1/1`, policy composition `1/1`, restart runtime `2/2`, cancellation `1/1`, exact absence `1/1`, sandbox restart `7/7`, sandbox live absence `1/1`, and fresh-process recovery `1/1` pass. Machine passes `39/39`, and node passes `67/67`. |
| R4 full affected behavior | Workloads pass `167/167`. Compute passes `297` with one declared ignore. Sandbox passes `1,012` with `43` declared ignores. Machine passes `39/39`. Node passes `67/67`. The serial Server lane passes `691` with `32` declared ignores. CLI passes `947` with one declared ignore. |
| R4 static convergence | NNCV006, NNCV008, NNCV015, NNCV021, NNCV033, and NNCV034 pass. NNCV033 passes `40/40` plus `50/50` mutations. NNCV034 passes `71/71` sole-diagnostic mutations. The aggregate passes `35/35` and `398/398`. The exhaustive aggregate child reached its final success result after the initial 30-minute outer controller proved too short; the controller changed no test or result. The censuses retain `66` bind authorities, `34` classified risks, and `114` exact composition keys. |
| R4 quality | Affected all-target checks, strict Clippy, warning-denied Rustdoc, Rustfmt, Prettier, JavaScript syntax, Bash syntax, scoped ShellCheck, diff, SDK build/test/typecheck, docs `108`, site `17/17`, and current-source modularity pass. The changed-Rust census has `13/13` exact threshold dispositions and one strong inherited exception. Known vendored Brotli warnings are unchanged. The active proof passes the technical-writing linter with zero diagnostics. The inherited plan has no new diagnostics in the changed recovery or ledger rows; its existing document-wide lint debt remains outside this item. |
| R4 candidate freeze | The pre-ledger staged tree is `63488a338db76cc65baea73afee257f0af46a00d`. The complete `218`-path item patch has SHA-256 `5cad6e9616c8fbb2f391fc67ecc76b293a42962ec981552f3199e52264aa6e0b`. Its complete executable/script patch has SHA-256 `de61345c0f81622d9d6a14aa988a8291c2af490f6fd07e6fd430caa060691bae`. The R4 `109`-path staged patch has SHA-256 `cb7911351a31fcce1d8d5ae88ce2968d4ad80f652e844df285f223af6b9e82bc`. Zero unstaged or untracked paths existed at freeze. |
| Full item review | The one full GPT-5.6 Sol/xhigh/fast review completed as five internal bundle passes over synthetic commit `f022911048053d37ff692e64a41181022b4eeae5`. It reported `22` findings and overall confidence `0.99`. Thread IDs are `019fe370-659c-7b72-b729-21a100d41161`, `019fe379-8f4e-7002-acd1-0d12e5768476`, `019fe37e-4ecd-77f0-a6c4-1827d231d228`, `019fe382-cd18-7af0-b3dd-1e8808eb6cad`, and `019fe388-d5b0-7932-8621-dc2b83cb6425`. All `22` findings have source-backed dispositions below. |
| Review cadence | The one full review is complete. No second broad review is authorized. Accepted material executable corrections require affected proof reruns and exactly one narrow correction review. Docs or ledger corrections do not trigger review. |
| Next action | Complete the three disjoint accepted-correction lanes, rerun affected A1-A20 proofs, freeze the corrected candidate, and run the one permitted narrow correction review. |

## Full Review Finding Disposition

We checked all `22` findings against the production call graph, durable state
transitions, provider adapters, and executable proof gates. We accepted all
findings within NNC6.4a. Three findings have evidence amendments that narrow
the correction to the actual defect. No finding starts NNC6.5 work.

| ID | Priority | Disposition | Verified defect and correction boundary |
| --- | --- | --- | --- |
| R4F01 | P1 | Accepted | Intrinsic activation-prerequisite, readiness, and publication-observation steps receive execute-mode claims. They must first persist `DispatchPending -> InspectionRequired`, then emit only an inspect command. |
| R4F02 | P2 | Accepted | Forwarded parent inspection mutates port-lease and listener state. Inspection must compare exact durable and live parent evidence without activation, withdrawal, recovery, or rebind effects. |
| R4F03 | P2 | Accepted | The guest journal can reuse execute-time absence as terminal inspection evidence. Execute absence must remain ambiguous until one exact inspection records absence. |
| R4F04 | P3 | Accepted | The contention test executes sequentially. A two-party synchronization point must force both admissions to load the same revision before competing CAS operations. |
| R4F05 | P1 | Accepted | One retained candidate failure becomes a fatal supervisor error on the next sweep and stops the global watch. Per-key failure must remain visible without terminating or starving the watch. |
| R4F06 | P1 | Accepted | The dedicated restart runtime enables time but not Tokio I/O, although injected store and provider futures can require the I/O driver. The runtime must enable the full required driver set. |
| R4F07 | P2 | Accepted with evidence amendment | Exact systemd `NoSuchUnit` after an effective ambiguous stop cannot complete quiescence recovery. Only explicit provider absence, not generic inactive/dead state, may authenticate stopped completion. |
| R4F08 | P2 | Accepted | The public node restart claim accepts equal or skipped confirmed revisions. A node-local command mode must enforce the exact checked execute or inspect revision relation and effect-method mode. |
| R4F09 | P3 | Accepted | Restart tests use arbitrary scheduler-yield budgets. Semantic notifications, barriers, or bounded acknowledged waits must replace them. |
| R4F10 | P3 | Accepted as proof defect | The local constant closures do not test GET or logical resolution. Delete the false proof and bind the gate to the existing real server GET and services resolution tests with zero provider effects. |
| R4F11 | P1 | Accepted | Krun execute replay at durable `NetworkAttached` only inspects. A fresh backend cannot recreate process-local PEP readiness. Execute must inspect first and idempotently reconverge only missing retained state. |
| R4F12 | P2 | Accepted with evidence amendment | Normal restart activation can promote an adopted target. The actual wedge is authorized teardown of an adopted, never-spawned prepared target, which repeatedly fails on a missing PID. Provider cleanup must handle that exact state without saga rollback. |
| R4F13 | P3 | Accepted with evidence amendment | The port fixture drops its socket before durable lease reservation. Retain the guard through reservation, then release it before the deliberate provider-bind race. |
| R4F14 | P1 | Accepted | Server withdrawal inspection trusts absence from a volatile running map. Success must require exact durable stopped-binding and retained-lease evidence for every plan member. |
| R4F15 | P1 | Accepted | A higher-generation successor is rejected after no-effect advancement, resolved effects, or definite failure. A durable successor-veto state must forbid new effects, retain partial evidence, and require inspection for issued ambiguity before NNC6.5 takes over. |
| R4F16 | P2 | Accepted | Only the latest completed idempotency key is durable. Use a strict bounded ordered per-generation history with no eviction, exact validation, full-history replay, and fail-before capacity exhaustion. |
| R4F17 | P3 | Accepted as docs defect | The proof scope and A20 closeout text still described an earlier checkpoint. They must show that the full review ran and corrections reopened the candidate. |
| R4F18 | P2 | Accepted | NNCV034 checks result tokens but not outcome-arm mapping. It must bind ambiguous and in-progress outcomes to inspection, and authenticated absence to inspect-mode retry only. |
| R4F19 | P2 | Accepted | NNCV034 checks driver token membership but not claim/effect/result order. It must require dispatch before result reduction and result CAS, with reorder mutations. |
| R4F20 | P2 | Accepted | NNCV034 has no negative census for a second restart decision or write authority. It must allow portable workloads transitions and exactly one compute coordinator path while rejecting upper or provider owners. |
| R4F21 | P2 | Accepted | The obsolete provider-scheduler scan covers only sandbox. It must also cover services, server, node, and CLI/machine provider owners while excluding the valid compute/workloads schedule seam. |
| R4F22 | P2 | Accepted | `hasTestsAt` accepts identifier substrings and empty unannotated functions. It must require an attributed exact Rust test with a nonempty outcome assertion, plus negative mutations. |

The correction candidate is not frozen. Each lane must first add the named
fail-before proof, make the smallest coherent correction, and pass its focused
and full affected gates. Root then reruns A1-A20 as affected, freezes one new
candidate, and runs exactly one narrow Sol/xhigh/fast review over the accepted
executable corrections. A second broad item review is forbidden.

## R4 And Accepted-Correction Path Scope

R4 added six source-derived proof and verifier paths. The accepted corrections
also added the exact services read-surface fixture and the exact server-store
recovery fixtures that exposed stale format and digest expectations. The final
R3-and-later allowlist contains all `151` changed paths. The audit checkpoint
differs from the complete item on `221` paths.

The bind-owner inventory and production network-authority census have only
source-line updates. They retain `66` bind authorities, `34` classified risks,
and `114` composition keys. The services fixture removes obsolete provider
policy and schedule fields. It adds no runtime effect. The final correction
adds the exact restart-dispatch test path and one assertion-parser sibling. No
path adds a new product authority, classification, provider realm, or
dependency edge.

The original six R4 additions remain:

- `docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json`.
- `docs/private/plans/proof/nimbus-network-control-plane/nnc4.6f-production-network-authority-census.json`.
- `scripts/nimbus-network-control-plane/workload-provision-dispatch-contract.sh`.
- `scripts/nimbus-network-control-plane/workload-provision-dispatch-self-test.sh`.
- `scripts/verify-nimbus-network-machine-forwarded-batch-convergence.mjs`.
- `scripts/verify-nimbus-network-control-plane.sh`.

The correction-specific integration additions are:

- `crates/nimbus-node/src/reconciler.rs`.
- `crates/nimbus-services/src/manager/tests/mod.rs`.
- `crates/nimbus-server/src/workload_saga_store/tests/composition.rs`.
- `crates/nimbus-server/src/workload_saga_store/tests/restart_process.rs`.
- `scripts/nimbus-network-control-plane/workload-restart-test-assertion.mjs`.

## Accepted-Correction Convergence

| Proof area | Current evidence |
| --- | --- |
| Full-review corrections | All `22` findings have source-verified corrections. Intrinsic steps inspect before effects; forwarded inspection is read-only; guest absence requires exact inspection; admission races overlap; one candidate failure cannot stop the watch; the runtime enables I/O; systemd and node claims use exact absence and revision modes; tests use semantic synchronization; real GET and logical-name tests replace the false local proof; Krun repairs fresh-process PEP state and cleans adopted never-spawned targets; the port race retains its socket through reservation; server withdrawal requires durable retained evidence; successors veto new effects but preserve issued evidence; completed idempotency history is bounded and non-evicting; and NNCV034 closes all five false-negative classes. |
| Corrected behavior | Full suites pass workloads `171`, compute `301 + 1 ignore`, sandbox `1,004 + 27 ignores`, node `70`, server `692 + 32 ignores`, CLI `948 + 1 ignore`, and services `82`. The exact server GET and services logical-name tests pass `1/1` each. Focused command-mode, contention, failure-isolation, process-loss, cleanup, lease, replay, fencing, and race tests pass. |
| Corrected quality | All eight affected crates pass all-target check, strict Clippy, and warning-denied rustdoc. SDK build, test, and typecheck pass with `24`-route parity. Rustfmt, Prettier, JavaScript syntax, Bash syntax, scoped ShellCheck, and diff checks pass. Docs pass `108`; the site gate passes `17/17`. Only the unchanged vendored Brotli warnings remain. |
| Corrected static contracts | NNCV033 passes `40/40` direct checks and `50/50` mutations. NNCV034 passes live and `80/80` mutations. The first aggregate replay failed only two stale source-line fixtures in NNCV006 and NNCV015. Both inventories are corrected. The frozen live verifier passes `35/35`, and the one replacement aggregate passes `407/407`. Its log is `/tmp/nnc64a-frozen-selftest.GE0rCd/output.log`. |
| Corrected candidate freeze | The pre-ledger staged tree is `090a9eecc96f95d2e0fc2c3da65e5cd309114f82`. The complete `220`-path item patch has SHA-256 `d6ac954fe23f1180e88643a85ee6fbd8c7268de1074aa59e206467eb73aeac95`. Its complete executable/script patch has SHA-256 `2d4a608aa084ffbdcad07dbccb977478185ef42492a30fbc573e5b5ef152f28e`. The `47`-path correction patch from review snapshot `f022911048053d37ff692e64a41181022b4eeae5` has SHA-256 `1df8d49325330a920065ade6eca4de9e7594b7c0048c706f0afac9c021e4fb9b`; its executable/script SHA-256 is `26a5814227f40e6fe421d3029b430ebf52af2fa3a90ed9be8f763dac0033275d`. The candidate had zero unstaged or untracked paths at freeze. |
| Narrow correction review | The one permitted GPT-5.6 Sol/xhigh/fast correction review ran against synthetic commit `73247dfc846b1b8c2b799b3c33e84428c4f29a08`. It reported four accepted findings at confidence `0.99`. The findings reopen only their affected proofs. Review cadence is exhausted; no further structured review is authorized. |

## Narrow Review Finding Disposition

| ID | Priority | Disposition | Verified defect and correction boundary |
| --- | --- | --- | --- |
| R5F01 | P1 | Accepted | Authenticated absence at `ObservePublication` repeats read-only inspection forever after fresh-process parent-lifetime loss. The reducer must return to the exact effect-owning `Publish` transition, then observe again. |
| R5F02 | P2 | Accepted | A successor race can terminally accept execute-time absence. Execute absence must remain inspection-required until an exact inspect command authenticates absence. |
| R5F03 | P2 | Accepted | Node inspection claims allow only `issuing + 2`, but later successor revisions can validly advance a vetoed read-only inspection. The host claim must authenticate explicit inspect mode and complete veto fences at the later revision without widening execute authority. |
| R5F04 | P2 | Accepted | NNCV034 accepts any nonempty test body, including declarations, helper-only calls, and tautological assertions. It must require a meaningful observable-outcome assertion in code and add exact negative mutations. |

Three disjoint lanes own these corrections. Compute owns R5F01-R5F02. Node
owns R5F03. NNCV034 owns R5F04. Each lane adds fail-before behavior and runs
focused plus full affected gates. Root then reruns the affected static and
quality proofs and closes the item without another structured review.

## Narrow-Review Correction Closeout

| Proof area | Final evidence |
| --- | --- |
| R5F01 | Exact publication-observation absence is durable authorization for one `ObservePublication` Inspect -> `Publish` Execute transition at the next dispatch epoch. Publish success returns to `ObservationPending`, and the next exact command is `ObservePublication` Inspect. Generic same-step absence retry rejects observation claims. |
| R5F02 | Execute-time absence always persists `InspectionRequired`, including when a later generation wins the result race. A stale execute result fails against the later fence. Only the exact inspect command can persist terminal `SuccessorVetoed` absence. |
| R5F03 | The confirmed compute command, machine wire, guest adapter, and node claim carry the exact optional successor-veto generation. Execute accepts only `issuing + 1` and no veto. Inspect without a veto accepts only `issuing + 2`. Inspect with a later veto accepts `issuing + 2` or a later confirmed revision, remains read-only, and rejects a crossed or non-later generation. |
| R5F04 | NNCV034 requires a meaningful observable-outcome assertion in the exact attributed Rust test. Six new mutations reject helper-only, declaration-only, identifier-only, tautological, comment-only, and string-only proof bodies. The suite passes `86/86`. |
| Affected behavior | Workloads pass `172/172`; compute passes `303` with one declared child-only ignore; machine passes `34/34`; node passes `72/72`; and CLI passes `948` with one declared machine-probe ignore. The unchanged full sandbox, server, and services evidence remains `1,004 + 27 ignores`, `692 + 32 ignores`, and `82/82`. |
| Affected quality | The correction crates pass all-target check, strict Clippy, and warning-denied rustdoc. Rustfmt, Prettier, JavaScript and Bash syntax, scoped ShellCheck, and diff checks pass. Only unchanged vendored Brotli warnings remain. |
| Review closeout | The one full review and one narrow correction review are complete. The four accepted narrow findings are corrected and source-verified. No third review ran or is warranted. |

## Final Current-Source Modularity Disposition

The R3-and-later diff contains `129` changed handwritten Rust files. Exactly
`19` are at or above 1,500 lines. The broader composition-owner census adds
one unchanged threshold owner, `port_lifecycle.rs`, so this table has `20`
rows. The one file above 2,000 lines has a strong inherited concept-owned
exception. All other files have an explicit ownership reason and remain below
2,000 lines.

The NNCV034 production scanner is `1,995` lines and retains one explicit deep
verifier ownership reason. The directly related assertion-shape parser moved
to the concept-owned `workload-restart-test-assertion.mjs` sibling at `151`
lines. The extraction adds no scanner, mutation, or product authority.

| Path | Lines | Ownership disposition |
| --- | ---: | --- |
| `crates/nimbus-sandbox/src/backends/container/runtime/runner.rs` | 2,085 | Strong inherited exception: one prepared-runner handoff, lifecycle-ownership, and cleanup-convergence state machine must preserve one decision record and lock order. The file was `2,086` lines before R4 and shrank by one line. Identity, recovery, and test-probe concerns already live in concept-owned children. NNC6.4a adds no responsibility. A future change to cleanup convergence must extract that intact phase before other growth. |
| `crates/nimbus-sandbox/src/backends/container/runtime/launch_cleanup.rs` | 1,991 | One initial-launch compensation state machine preserves artifact cleanup and manifest finality in one ordered transition. The file was `1,993` lines before R4 and shrank. Extract a complete compensation phase before this owner reaches 2,000 lines. |
| `crates/nimbus-workloads/src/saga/tests.rs` | 1,961 | One portable saga-invariant test root. Provision-state and restart-state cases live in concept-owned children. The file has no production authority and no R4 growth. |
| `crates/nimbus-server/src/tests/service_manager.rs` | 1,716 | One server service-manager integration-test root. Restart cases live in `tests/service_manager/restart.rs`. The correction adds only shared fixture evidence and no production authority. Extract the next complete route-test family before 2,000 lines. |
| `crates/nimbus-workloads/src/saga/state.rs` | 1,667 | One portable saga transition composition root. Restart invariants and transitions live in `saga/state/restart.rs`. The correction routes bounded history and successor-veto validation without provider effects. |
| `crates/nimbus-cli/src/machine/backend/provision/tests.rs` | 1,658 | One parent-host provision-adapter contract tests exact journal, lease, fencing, and replay behavior. Restart-specific mapping tests live in `provision/restart/tests.rs`. Extract another complete adapter-test family before 2,000 lines. |
| `crates/nimbus-node/src/reconciler.rs` | 1,653 | One host-local node reconciliation composition root. Restart command authentication lives in `host_lifecycle/restart.rs`. The correction updates an inspection fixture and shrinks this file; it adds no authority. |
| `crates/nimbus-cli/src/machine/backend/provision.rs` | 1,632 | One parent-host exact-phase adapter authenticates compute-confirmed commands and owns parent ingress lease/lifetime reconciliation. Restart-specific mapping and tests live in the `provision/restart.rs` and `provision/restart/tests.rs` children. Compute owns lifecycle order. The guest owns workload and forwarding effects. Extract the intact parent-publication state machine before this owner reaches 2,000 lines. |
| `crates/nimbus-workloads/src/saga.rs` | 1,620 | One portable saga vocabulary and composition root. Executable, provision, restart, state, network, and test behavior live in concept-owned children. It owns no provider effect. |
| `crates/nimbus-sandbox/src/backends/container/runtime/machine_port_publication.rs` | 1,606 | One external-publication journal and exact command/authority authentication state machine. NNC6.4a adds planned-lease withdrawal and authenticated-absence reconciliation. Machine transport and provider selection remain outside. |
| `crates/nimbus-sandbox/src/backends/oci/port_lifecycle.rs` | 1,589 | One OCI port transition state machine. The machine-specific behavior remains in the concept-owned `port_lifecycle/machine.rs` child. |
| `crates/nimbus-sandbox/src/backends/container/runtime/restart.rs` | 1,582 | One Container restart-capability state machine owns exact retained withdrawal, reset, attachment, activation, readiness, and evidence checks. Its behavior tests live in `restart/tests.rs`. Splitting the ordered state machine would weaken local invariant visibility. |
| `crates/nimbus-sandbox/src/backends/krun/vm/tests.rs` | 1,578 | One Krun VM behavior-test root with concept-owned children for attachment, recovery, lifecycle, restart, and fencing cases. It has no production authority and shrank during R4. |
| `crates/nimbus-sandbox/src/backends/container/runtime.rs` | 1,576 | One Container backend composition root. Artifact, attachment, effect, inspection, machine-port, provision, restart, runner, status, support, and tests live in concept-owned children. The root gained no lifecycle authority. |
| `crates/nimbus-workloads/src/store/tests.rs` | 1,565 | One portable store-conformance test root. Restart-candidate query behavior lives in its concept-owned child. It has no production authority and no R4 growth. |
| `crates/nimbus-machine/src/api/tests.rs` | 1,617 | One strict machine wire-contract test owner covers provision and restart serialization, unknown-field rejection, crossed fences, and response correlation. It has no production authority. Extract one complete wire family before 2,000 lines. |
| `crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs` | 1,516 | One Krun VM lifecycle owner coordinates exact process, namespace, and cleanup state. Restart-specific policy and dispatch stay in `vm/restart.rs`. The correction adds safe adopted never-spawned cleanup without a second lifecycle authority. |
| `crates/nimbus-sandbox/src/backends/container/runtime/machine_ports.rs` | 1,508 | One host machine-proxy registry and cleanup-lifecycle owner. Durable publication authentication stays in `machine_port_publication.rs`. No unrelated provider or transport selection enters this file. |
| `crates/nimbus-server/src/workload_ingress.rs` | 1,507 | One server ingress publication adapter owns durable retained-listener evidence and provider-local reconciliation. The correction makes inspection read-only and exact; it does not move allocation or policy authority into this owner. Tests live in `workload_ingress/tests.rs`. |
| `crates/nimbus-workloads/src/saga/state/restart.rs` | 1,504 | One portable restart state-machine and transition validator owns phase, disposition, absence authorization, successor veto, and owner-observation invariants without provider effects. The observation-absence correction adds one exact reverse transition. Split only a complete invariant family before further growth. |

## Audit Acceptance Traceability

| Clause | Audit status |
| --- | --- |
| A1 | Green. The source census, call graphs, bind inventory, composition census, and exact path allowlists cover the complete correction diff. Line-only metadata refreshes add no authority. |
| A2-A3 | Green. The closed portable vocabulary and nested same-generation state preserve exact execution-attempt and publication fences. Format v5 strictly owns bounded completed-request history. |
| A4-A6 | Green. Compute remains the sole restart writer. Execute-time absence persists inspection before a successor veto can become terminal. |
| A7-A13 | Green. Observation absence re-enters the exact effect-owning publish transition, later-veto inspection crosses the machine boundary without effect authority, and all generations, attempts, revisions, and dispatch epochs remain fenced. |
| A14-A17 | Green. Service, SDK, machine, cancellation, runtime-I/O, real GET, and logical-name surfaces pass. Reads remain effect-free, and durable submitted work survives waiter cancellation. Node inspection fencing is tracked under A7-A13. |
| A18 | Green. Fresh-process observation, successor execute-absence, later-veto node inspection, machine-wire, guest mapping, and assertion-quality fail-before cases pass with the exact counts above. |
| A19 | Green for the strengthened NNCV034 contract at `86/86`; final aggregate evidence is recorded in the closeout checkpoint below. |
| A20 | Green for affected behavior and quality. The one full and one narrow review are complete; review cadence is exhausted. Final docs and aggregate evidence are recorded in the closeout checkpoint below. |

## Final Closeout Checkpoint

| Gate | Exact result |
| --- | --- |
| Narrow-review defects | R5F01-R5F04 have fail-before, corrected behavior, full affected-suite, source-contract, and boundary evidence. No accepted finding remains open. |
| Static restart contract | NNCV034 passes `86/86`, including the six assertion-quality mutations. |
| Complete aggregate | `bash scripts/verify-nimbus-network-control-plane.sh --self-test` exits `0` and ends `self-test: 413 passed, 0 failed`. The complete log is `/tmp/nnc64a-final-aggregate-selftest.log`. |
| Live aggregate | `bash scripts/verify-nimbus-network-control-plane.sh` passes `35/35`. |
| Behavior | Workloads pass `172/172`; compute passes `303` with one declared child-only ignore; sandbox passes `1,004` with `27` declared ignores; machine passes `34/34`; node passes `72/72`; server passes `692` with `32` declared ignores; CLI passes `948` with one declared machine-probe ignore; and services pass `82/82`. |
| Quality | Affected all-target checks, strict Clippy, warning-denied rustdoc, Rustfmt, diff checks, JavaScript and Bash syntax, Prettier, scoped ShellCheck, SDK build/test/typecheck with `24`-route parity, and proof prose lint pass. Only unchanged vendored Brotli warnings remain. |
| Documentation | `scripts/check-docs.sh` passes `108` pages. `scripts/verify-nimbus-docs-site.sh` passes `17/17`. |
| Scope and modularity | The complete item has `221` paths; R3-and-later has `151`. The final threshold census has `129` changed handwritten Rust files, `19` changed files at or above 1,500 lines, and the `20`-row broader composition-owner table above. NNCV034 remains one scanner/mutation authority with its concept-owned assertion parser. |
| Review cadence | The one full Sol/xhigh/fast item review and one narrow Sol/xhigh/fast correction review are complete. The accepted executable corrections are proven. No third review ran or is warranted. |
| Acceptance | A1-A20 are green. NNC6.4a is complete and ready for its exact ledger/evidence commit. NNC6.5 remains the sole next teardown owner. |
