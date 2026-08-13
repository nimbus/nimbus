# NNC6.5g final teardown convergence

Status: `complete; durable item checkpoint is the commit containing this proof`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

## Recovery checkpoint

| Field | Value |
| --- | --- |
| Dependency | NNC6.5e, NNC6.5f2, and NNC6.5f3 are complete. NNC6.5f3 item commit `90839aa1cf0f93c1a0363f226969f30166e85858` is the clean item base. |
| Current state | G1-G28 are green. The four full-review corrections and two narrow-review recovery corrections pass their focused, affected, static, and documentation gates. Review cadence is exhausted. |
| Owned outcome | Route every remaining teardown caller through the existing durable compute saga, close exact compensation races, delete every frozen legacy bypass, and make NNCV035 green. |
| Owned paths | The exact product, deletion-only, test, verifier, and control-plane sets below. The current dirty product paths are attributed to the three isolated implementation seams named above. |
| Forbidden scope | Do not move provider effects, service naming, tenant policy, proxy behavior, or cluster transport into `nimbus-network`. Do not weaken retained ambiguity or cleanup fences. |
| Last green | Final affected suites pass: Workloads `224/224`; Compute `445 passed, 1 ignored`; Services `89/89`; Node `121/121`; Sandbox `1,152 passed, 29 ignored`; Server `731 passed, 35 ignored`; CLI `1,007 passed, 4 ignored`. Strict affected Clippy and warning-denied Rustdoc, formatting, diff, Bash/Node syntax, docs `108`, site `17/17`, NNCV035 direct/native/physical `1/1` each, helper `172/172`, live architecture `36/36`, and aggregate mutations `556/556` pass. |
| Next action | Commit this exact owned item, reconcile current main from the clean checkpoint, and start NNC6.6's read-only service-resolution audit. Do not run another NNC6.5g structured review. |
| Blocker | None. |

## Frozen source result

The live NNCV035 contract is exact `0 passed, 4 failed`. Its only diagnostics
are `service`, `tenant`, `compensation`, and `behavior`. The focused helper is
`150/150`; the aggregate mutation suite is `552/552`; and the live
architecture verifier is `35/36`, with NNCV035 as its sole red condition.

The audit confirmed two missing product transitions:

1. A provision `DefiniteFailure` is durable, but compute projects and returns
   it without first committing `WorkloadTeardownCause::FailedProvision` and
   driving the retained resources through the existing teardown runtime.
2. If a restart `DefiniteFailure` becomes durable before a later stopped
   successor, retirement remains pending. The reverse order already settles
   correctly. The fix must hand the exact retained result to teardown; it must
   not invent success or add an automatic failed-restart retirement policy.

The audit also confirmed duplicate legacy authority:

- `nimbus-services/src/manager/retirement.rs` still inspects and stops sandbox
  providers and performs tenant-wide provider cleanup.
- `TenantServiceRetirement` still enumerates process-local projections instead
  of durable workload sagas.
- `HostLifecycleBackend::stop` lets the node reconciler perform a second stop.
- `SandboxBackend::stop` remains a coarse lifecycle capability after exact
  execution and network teardown adapters replaced it.
- The direct services definition-delete API retains the obsolete mutation gate
  and calls the legacy retirement effect path. Product routes already use the
  compute-owned source claim and recorded-only finalizer.

No provider adapter needs new compensation policy. Provision and restart
ambiguity already stays inspection-only. `nimbus-network` remains unchanged.

## Frozen target choreography

### Failed provision

The retained provision worker must complete this exact sequence before it
publishes a definite-failure result or removes its keyed task:

```text
exact provider result -> durable DefiniteFailure
  -> exact FailedProvision cause CAS
  -> WithdrawalCommitted
  -> existing WorkloadTeardownRuntime
  -> reverse only evidence-proven effects
  -> Recorded or truthful pending/cleanup state
```

A concrete private compute compensation owner will compose the existing
`WorkloadSagaCoordinator` and `WorkloadTeardownRuntime`. It is not a trait or a
provider abstraction. A cause-CAS conflict or ambiguous response requires an
exact read and classification. Provider ambiguity never authorizes
compensation. Cancellation after tracked submission detaches only the waiter;
the retained worker continues this sequence.

Managed provision is invalid without the exact teardown realm. Composition
must fail before source reservation, saga mutation, or provider effect. The
same concrete teardown runtime serves normal resource retirement,
failed-provision compensation, and tenant retirement.

### Restart handoff

The portable state accepts an exact terminal restart result for teardown when
either legal ordering occurs:

```text
stopped successor -> inspect issued restart -> terminal SuccessorVetoed
terminal DefiniteFailure -> stopped successor retains the exact terminal result
```

Both orders must retain the claim, source and target execution references,
owner observations, result, and successor fence before
`WithdrawalCommitted`. A restart failure without a stopped successor remains a
restart failure and does not start teardown.

### Tenant retirement

Explicit live-process tenant deletion uses one compute-owned driver:

```text
Engine begin-delete fence
  -> services tenant-source barrier bound to tenant incarnation
  -> runtime-owner retirement
  -> bounded durable child-saga inventory
  -> exact per-key compensation or stopped-successor teardown
  -> second complete inventory and all-Recorded/stopped proof
  -> effect-free services source/session finalization
  -> Engine finish-delete
  -> release the process-local services barrier
```

The driver derives service versus standalone identity from the durable
`WorkloadProvisionSourceIdentity`. It never uses an address or an observed
projection as identity. The source barrier rejects new definition create or
update, source prepare or reservation, and session admission. It survives an
unsuccessful delete attempt in the current process. Work admitted before the
barrier is joined, and any exact late result is retired.

The final inventory detects a child inserted during the first pass. Store
unavailability, corrupt or crossed pages, cursor regression, missing source
identity, unresolved restart or provision, `CleanupPending`, teardown pending,
or finalizer failure keeps Engine finish fenced. Successful sibling progress
remains durable for retry.

Services finalization receives the exact complete recorded inventory. It
authenticates source kind, stable name, source generation, resource version,
terminal stopped intent, uniqueness, and tenant before it changes manager
state. It removes definitions, sources, observations, sessions, and source
claims without inspecting or stopping a provider. Per-workload teardown owns
known provider release. NNC8 retains orphan discovery and cleanup; NNC6.1e2
retains fresh-process discovery of an interrupted tenant deletion and final
startup convergence.

## Exact path ownership

Primary compute paths:

- `crates/nimbus-compute/src/workload_saga.rs`.
- new `crates/nimbus-compute/src/workload_saga/provision_compensation.rs` and
  concept-owned tests.
- new `crates/nimbus-compute/src/tenant_retirement.rs` and concept-owned tests.
- `crates/nimbus-compute/src/workload_provisioner.rs` and its tests.
- `crates/nimbus-compute/src/workload_saga/restart_runtime.rs` and its tests.
- `crates/nimbus-compute/src/resource_provision.rs`,
  `resource_retirement.rs`, and their concept-owned tests.
- `crates/nimbus-compute/src/state.rs`, `config/node_services.rs`, and `lib.rs`.

Primary services paths:

- `crates/nimbus-services/src/manager.rs`, `manager/types.rs`,
  `manager/definitions.rs`, `manager/source.rs`, `manager/source_retirement.rs`,
  `manager/sessions.rs`, and `lib.rs`.
- delete `manager/retirement.rs` and `manager/definition_mutation.rs` after
  their replacements pass.
- replace `manager/tests/tenant_teardown.rs`; remove obsolete direct-retirement
  cases from `manager/tests/sandbox_resources.rs` and
  `manager/tests/definition_lifecycle.rs`; add concept-owned barrier and
  finalization tests.

Required narrow portable amendment:

- `crates/nimbus-workloads/src/saga/state/teardown.rs`, the exact outer-state
  validation guard in `saga/state/restart.rs`, and their exact restart handoff
  tests only. The fail-before test proved that a one-CAS handoff must extend
  both validators: an unpersisted intermediate rewrite would fail store
  `validate_successor`. These edits accept only the terminal
  result-before-successor ordering with the same settlement equality checks.
  They add no provider effect or new teardown cause.

Composition and integration paths:

- `crates/nimbus-server/src/workload_composition.rs` and tests only to require
  exact teardown composition and prove Engine-backed tenant ordering.
- server saga-store process tests only for real reopen proofs.
- NNCV035 fixture, source contract, and attributed-test assertion files.

Deletion-only handoff paths:

- `crates/nimbus-node/src/reconciler.rs`, `host_lifecycle.rs`,
  `direct_process.rs`, `systemd_transient.rs`, their tests, and CLI test
  implementations of `HostLifecycleBackend` may only remove coarse stop
  authority or assert observed projection behavior.
- `crates/nimbus-sandbox/src/backend.rs`, Container and Krun coarse-stop roots,
  ForwardedMachineApi sandbox trait implementations, and every affected test
  double may only remove `SandboxBackend::stop` or move an existing cleanup
  test to the exact teardown adapter. Provider journals, exact execute/inspect
  methods, launch compensation, and attachment release behavior do not change.
- Mechanical constructor callers may pass `SandboxBackendKind` after
  `ServiceManager` drops its retained provider capability.

Any newly discovered implementation of either coarse stop trait joins this
mechanical deletion handoff. It cannot add behavior. The final proof records
every such path. No compatibility shim, default stop, or hidden test-only
coarse stop is allowed.

G25 deleted `SandboxBackend::stop` from the trait, every real and test
implementation, Container's coarse `stop_sync`, and Krun's `coarse_stop.rs`.
It also removed the superseded Krun attachment-recovery stop module. Legacy
tests now use their owning launch-compensation, runner-handoff,
provider-failure, terminal-IPAM, or read-only inspection seam. The exact
execution and network teardown adapters and journals remain the only teardown
effect authority. Static scans find no sandbox-shaped `fn stop`, `stop_sync`,
`coarse_stop`, compatibility shim, or test-only broad stop; the remaining
`.stop()` names are exact teardown-state accessors or unrelated process/proxy
concepts.

G26 replaces joined-crate token checks with product-local, body-local dataflow
checks for tenant pagination and per-key driving, all-recorded finalization,
failed-provision cause/result linkage, ambiguous inspection, retained-worker
cancellation, restart settlement, and exact test attribution. Eighteen paired
mutations prove those nine contracts. The corrected focused helper is
`172/172`; direct, native, and physical stages are each `1/1`; the aggregate
is `556/556`;
and the live architecture verifier is `36/36`. The closeout also updated stale
NNCV024/NNCV025 source coordinates and four unchanged composition-census line
numbers without changing their ownership classifications.

G27 closes the candidate with full affected behavior and static proof. Full
Workloads is `224/224`; Compute is `445 passed, 1 ignored`; Services is
`89/89`; Node is `121/121`; Sandbox is `1,152 passed, 29 ignored`; serialized
Server is `731 passed, 35 ignored`; and CLI is `1,007 passed, 4 ignored`.
Focused corrections add three CLI fixture passes, tenant retirement `10/10`,
restart settlement `1/1`, and the extracted Container planning-failure recovery
child `2/2`. Strict all-target/all-feature Clippy and warning-denied Rustdoc
pass for the eight affected crates; only unchanged vendored Brotli warnings
remain outside the warning-denied Nimbus crates.

The final static bundle passes NNCV035 direct, native, and physical stages at
`1/1` each, its mutation helper at `172/172`, the aggregate mutation harness at
`556/556`, and the live architecture verifier at `36/36`. NNCV004 and NNCV012
prove that `nimbus-network -> nimbus-core` remains the only initial workspace
edge and that no provider, policy, socket, transport, naming, cluster, or cloud
effect entered `nimbus-network`; NNCV008 proves plan recovery. Format, diff,
Node and Bash syntax, changed-helper ShellCheck, and aggregate ShellCheck with
its established SC2034/SC1091 exclusions pass. Documentation is `108` pages
and site verification is `17/17`. The focused proof-ledger lint reports `28`
unique ordered criteria and zero diagnostics.

The corrected final owned census has `138` paths, including `129` Rust paths.
No changed
handwritten Rust file is at or above `2,000` lines. Nine existing coherent
state-machine, composition, or test owners remain in the documented
`1,500-1,999` exception band. The only file that crossed `2,000` during G27,
Container `launch_cleanup.rs`, now has `1,852` lines after its intact two-test
planning-failure recovery concept moved to a `175`-line child. Temporary debug
instrumentation is absent; the added process `eprintln!` calls are intentional
PID synchronization evidence for crash-cut tests.

Control-plane paths are this proof, the canonical plan and plan index, and the
NNCV035 scripts. No `nimbus-network`, policy, service naming, proxy, cluster,
or system-projection product path is owned.

The sole narrow correction review used GPT-5.6 Sol with xhigh reasoning and
fast service tier. Its two internal threads were
`019ff804-09c6-7f83-bb91-f5b10ca1ad82` and
`019ff809-0219-75e2-b956-700dc35e6d64`. It reported two accepted P2 recovery
defects. A retained failed-provision compensation error is parked and cannot
retry from the exact durable run in the same process. An interrupted Krun
attachment adoption rejects the exact no-effect cleanup states
`ReservationCleanupPending` and `Absent`, so a crash during reserved cleanup
cannot converge. Both corrections received deterministic behavioral proofs.
The structured-review cadence is exhausted. No third review will run.

Both narrow-review corrections are complete. The retained compensation error
stores the exact boxed failed run, and the retained worker retries
`finalize_run` without calling the provision driver. The focused regression
`failed_provision_compensation_error_retries_exact_run_without_provision_effects`
passes `1/1`. The related full Compute suite passes `445` with one ignored
child entrypoint. Strict Clippy required boxed payloads for both retained-work
enum variants. The focused regression stayed green after that representation
change.

Krun now persists the exact attachment-to-segment association while adoption
is in progress. Final teardown authenticates that association when reserved
cleanup has already reached `ReservationCleanupPending` or `Absent`. The
same-process four-state matrix, fresh-process four-state/replay matrix, and
impossible cleanup-order test each pass `1/1`. One unrelated transient
`/usr/bin/true` process-inspection failure affected the first full Sandbox run.
The isolated retry passed. A clean full rerun passed `1,152` with 29 ignored
environment or child entrypoints.

NNCV035 now requires the retained failed-run shape and proves that retained
retry cannot call the provision driver. Its two focused mutations fail only
with the compensation diagnostic. Direct, native, and physical stages each
pass `1/1`. Helper mutations pass `172/172`. The complete aggregate passes
`556/556`. The live verifier passes `36/36`.

The Krun correction shifted four existing NNCV015 census coordinates by three
lines. The fail-closed census
caught all four shifts. The exact coordinates and focused plus live proofs now
pass.

## Acceptance ledger

| ID | Verifiable acceptance criterion | Status |
| --- | --- | --- |
| G1 | The dirty-state census attributes only the plan and this proof before product edits; exact base is `90839aa1cf0f93c1a0363f226969f30166e85858`. | `pass` |
| G2 | Direct NNCV035 is exact `0/4`; helper `150/150`, aggregate `552/552`, and live architecture `35/36` establish the fail-before state. | `pass` |
| G3 | A concrete compensation owner derives `FailedProvision` from the exact durable claim/failure and confirms its cause CAS by exact readback. Crossed evidence changes no bytes and makes no provider call. | `pass` |
| G4 | Eight provision-step failures compensate only evidence-proven resources in reverse order and finish `Recorded` or retain truthful pending/cleanup state. No resource-free step fabricates evidence. | `pass` |
| G5 | Provision Execute ambiguity and unresolved store CAS remain inspection-only. Compensation starts only after an exact durable definite result. | `pass` |
| G6 | Same-key contenders, replay, and an ambiguous cause CAS produce one cause transition and one provider-effect sequence. | `pass` |
| G7 | Cancellation after tracked submission detaches only the waiter; retained failed provision still commits and drives compensation before keyed work is removed. | `pass` |
| G8 | `failed_service_start_enters_durable_compensation_without_caller_stop` proves caller failure, exact durable cleanup, and zero services/caller coarse-stop effect. | `pass` |
| G9 | `failed_sandbox_start_enters_durable_compensation_without_caller_stop` proves the same contract for standalone sandboxes. | `pass` |
| G10 | Real subprocess cuts cover provision effect-before-result, result-before-cause, cause-response loss, and teardown effect-before-result using the same Engine and provider journal without duplicate effects. | `pass` |
| G11 | Exact pre-effect and adopted-never-spawned compensation cases do not wedge and do not fabricate execution, attachment, or publication effects. | `pass` |
| G12 | Managed provisioning without the exact teardown realm fails before source reservation, saga write, provider journal, socket, sandbox, or network effect. | `pass` |
| G13 | Both restart/successor orderings retain the exact terminal result before `WithdrawalCommitted`; ambiguous restart remains inspection-only; failure without a successor does not auto-retire. | `pass` |
| G14 | `restart_result_is_settled_before_withdrawal_committed` and a real reopen cut prove no duplicate restart Execute and exact settlement evidence through `Recorded`. | `pass` |
| G15 | A tenant-source barrier binds tenant ID and Engine incarnation, rejects new source/session/definition admission, and remains installed after any unsuccessful live-process deletion. | `pass` |
| G16 | `list_tenant_sagas` uses bounded immutable-key pagination, validates tenant/key/cursor truth, and drives each enumerated key exactly once per pass. | `pass` |
| G17 | `drive_tenant_teardown` compensates exact failed provision, settles issued provision/restart, persists a stopped successor where required, and uses the existing teardown runtime. | `pass` |
| G18 | `require_all_recorded_before_finish_tenant_delete` performs a second full inventory and rejects new, missing, crossed, duplicate, nonterminal, pending, or cleanup-pending children. | `pass` |
| G19 | Services finalization authenticates the complete recorded/stopped inventory before one atomic manager mutation and performs zero inspect, stop, or tenant-wide provider cleanup effects. | `pass` |
| G20 | `tenant_delete_waits_for_every_durable_workload_teardown_before_storage_delete` proves Engine finish follows child teardown and services finalization. | `pass` |
| G21 | Multi-page, concurrent-insert, sibling failure, retry, other-tenant isolation, unstarted source, orphan record, corrupt page, and tenant recreation cases pass without duplicate effects or premature deletion. | `pass` |
| G22 | Native service, sandbox, and definition routes retain all NNC6.5e source-fence, late-result, restart-settlement, session, and recorded-projection proofs. | `pass` |
| G23 | Services loses `TenantServiceRetirement`, direct retirement methods, the obsolete definition-mutation gate, retained `SandboxBackend` capability, and every provider effect. | `pass` |
| G24 | Node reconciliation becomes observed projection only; `HostLifecycleBackend::stop`, its implementations, and `NodeWorkloadReconcileAction::Stopped` are absent while exact drain/stop providers remain. | `pass` |
| G25 | `SandboxBackend::stop`, all implementations, and coarse Container/Krun stop entrypoints are absent. Exact teardown adapters, journals, launch compensation, and cleanup recovery remain green. | `pass` |
| G26 | NNCV035 uses body-local dataflow checks and 22 new mutations for projection, pagination, per-key driving, all-recorded order, cause/result connection, ambiguity, cancellation, restart settlement, mandatory compensation composition, retained ownership, and test attribution. Final helper is `172/172`; direct/native/physical are green; aggregate is exact `556/556`; live architecture is `36/36`. | `pass` |
| G27 | Dependency/effect scans prove `nimbus-network -> nimbus-core` is unchanged and no network/provider/policy/transport authority moved. Format, affected tests, strict Clippy/Rustdoc, docs, site, proof lint, and plan ledger checks pass with exact counts. | `pass` |
| G28 | After G1-G27 are green and the item is candidate-frozen, exactly one GPT-5.6 Sol/xhigh/fast item review runs. Accepted executable findings receive affected proofs and at most one narrow correction review. The exact owned diff and evidence are committed as one NNC6.5g item. | `pass` |

## Failure and recovery matrix

| Failure or race | Required result | Forbidden result |
| --- | --- | --- |
| Provision result is ambiguous | Persist/retain exact inspection requirement. | Cause commit or teardown effect. |
| Definite failure result is durable | Commit exact `FailedProvision` cause, then drive retained resources. | Projection-only failure or caller stop. |
| Cause CAS response is lost | Reload and authenticate exact durable bytes. | A second inferred cause or provider effect. |
| Cancellation follows submission | Waiter returns; retained worker continues. | Aborting durable compensation. |
| Restart result precedes stopped successor | Retain exact result and hand it to teardown after successor. | Permanent pending or fabricated success. |
| Stopped successor precedes restart result | Inspect exact issued restart and settle it before withdrawal. | Duplicate Execute or early withdrawal. |
| Tenant inventory is unavailable or corrupt | Keep Engine and services barriers; make no unconfirmed effect. | Treating the tenant as empty. |
| Child is inserted during retirement | Second inventory detects and retires or rejects it. | Engine finish with an orphan. |
| One child is pending or cleanup-pending | Retain sibling progress and fail closed. | Source purge, Engine finish, or reuse. |
| Services evidence is crossed or incomplete | Preserve all services state. | Partial source/session deletion. |
| Engine finish fails after services finalization | Keep tenant-source barrier for idempotent retry. | Re-admitting work into the deleted incarnation. |

## Routed boundaries

- NNC6.1e2 owns fresh-process discovery of interrupted tenant retirement,
  startup enumeration, and final convergence. NNC6.5g supplies a reusable
  exact-key/live-delete driver and durable child truth.
- NNC8.2 owns generic stale/live provision and restart claim recovery. This
  item preserves inspect-before-action and proves only its required exact-key
  crash cuts.
- NNC8.3 owns cleanup-pending finalization, orphan cleanup, and reuse. Tenant
  deletion cannot cross `CleanupPending`.
- NNC6.6 owns logical service resolution fencing. This item changes source
  admission and teardown only.
- Known per-workload provider effects remain in server, sandbox, node, machine,
  proxy, and CLI adapters. Cluster transport and `nimbus-network` remain out of
  scope.
