# NNC6.4a Fenced Restart Substitution Audit

Status: `audit complete; target and fail-before contract frozen; product source unchanged`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

Audit base: `c09ee6015ecd9164b98fa4d1f84bb26214ddedde`

## Scope

NNC6.4a installs the first production tenant-workload restart authority.
NNC5.6 made sandbox inspection side-effect-free. NNC6.4 deleted the old
service `stop` then `start` restart path. This audit defines the portable
restart state, compute coordination, provider commands, service surface,
failure behavior, and deletion gates before a product edit.

This checkpoint does not restart a workload, change a provider, add a route,
change the SDK, or alter the workload store. It records the expected-red
contract that the implementation must satisfy.

## Audit Result

Nimbus has no active production tenant-workload restart loop. This is the safe
intermediate state:

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

NNC6.4a can therefore add one authority directly. It needs no compatibility
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

## Current Source Census

The census is product source unless a row says `test-only`.

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

`WorkloadSagaIntent` gains a closed `WorkloadRestartPolicy` that is derived
from the admitted sandbox spec and covered by the desired digest. Compute must
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
   until the transition is confirmed.
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

NNCV034 is the first executable change after this audit checkpoint. Its live
contract must be expected red on the audit base and its mutation suite must be
green. It extends the existing source scanner and aggregate verifier rather
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
10. deletion of provider-local scheduler fields, dormant decision helpers,
    coarse restart routes, and network-crate effects.
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

No product edit starts until the helper, aggregate integration, exact summary
count, and expected-red proof are committed.

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
- `crates/nimbus-compute/src/workload_provisioner.rs` and tests only where the
  shared cancellation/tracked-work seam is generalized.
- `crates/nimbus-compute/src/services.rs` and tests.
- `crates/nimbus-server/src/workload_saga_store.rs`, its codec/schema/recovery
  children, and exact process tests.
- `crates/nimbus-server/src/http/services.rs`, `router.rs`,
  `workload_composition.rs`, `state.rs`, and focused tests.
- `crates/nimbus-sandbox/src/inspection.rs` and the pure classifier/tests.
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
- the NNCV034 helper, aggregate verifier, this proof, plan, and routing index.

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
| Review cadence | No structured review ran for this partial audit. NNC6.4a receives one review only after A1-A20 are green and the complete item is candidate-frozen. |
| Next action | Implement R0 only: add NNCV034, its fail-closed mutation suite, aggregate integration, exact expected-red evidence, and the source-derived allowlist. Commit that fail-before checkpoint before product changes. |

## Audit Acceptance Traceability

| Clause | Audit status |
| --- | --- |
| A1 | Frozen by the source census, evidence locations, and current call graphs. |
| A2-A6 | Target portable model and identity decisions are frozen; implementation is expected red. |
| A7-A13 | Target lifecycle, capability, race, clock, and deletion rules are frozen; implementation is expected red. |
| A14-A17 | Service, watch, node, machine, cancellation, and recovery contracts are frozen; implementation is expected red. |
| A18-A20 | Exact proof families, NNCV034 gate, quality gates, and one-review cadence are frozen; no completion claim is made. |
