# NNC6.5b Compute-Confirmed Teardown Driver

Status: `fail-before recorded; implementation pending`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

## Result

NNC6.5b adds the compute-owned command boundary between the portable teardown
reducer and later provider adapters. Compute confirms one durable candidate
and creates one fenced command. It selects one exact small capability and
invokes the injected capability. It confirms the result before it asks the
portable reducer for the next decision.

This item does not implement a provider adapter or cut over a product caller.
It does not edit `ComputeState`, start a background recovery scan, or install
an empty production registry. NNC6.5c and NNC6.5d implement the real
capabilities. NNC6.5e through NNC6.5g compose callers and delete old
authorities.

## Current And Target Seams

Current recovery duplicates the portable phase decision:

```text
durable record
  -> nimbus-compute recovery.rs phase switch
  -> raw withdraw/drain/stop/detach/release action

durable record
  -> nimbus-workloads decide_teardown
  -> strict attempt/claim/result reducer
```

The target has one decision authority:

```text
durable record
  -> nimbus-workloads decide_teardown
  -> nimbus-compute candidate builder
  -> existing coordinator confirmation
  -> confirmed Execute or Inspect command
  -> exact injected capability
  -> correlated provider observation
  -> portable result reducer
  -> exact successor confirmation
```

The `nimbus-network -> nimbus-core` dependency remains unchanged. Provider
effects remain in server, node, sandbox, and machine adapters.

## Source Audit

The read-only audit used item checkpoint
`eefbb7178e304450632f266bdfec35ada2a55d71` and recovery checkpoint
`ac1a6c80f2b4fa204ee443f5f72c23e7768983e3`.

| Finding | Evidence | Required NNC6.5b action |
| --- | --- | --- |
| Raw recovery duplicates teardown decisions. | `crates/nimbus-compute/src/workload_saga/recovery.rs` defines and derives raw withdrawal, drain, stop, detach, release, terminal, cleanup, and resource-free actions. | Replace the raw variants with one `Teardown(WorkloadTeardownDecision)` projection that delegates to `WorkloadSagaRecord::decide_teardown`. Do not keep compatibility variants. |
| The durable confirmation seam already exists. | `crates/nimbus-compute/src/workload_saga/provision_dispatch.rs` owns `WorkloadSagaConfirmation`, exact successor validation, one CAS, and one ambiguity read. | Reuse the existing seam. Do not copy CAS classification or ambiguity resolution. Physical neutralization is later modularity cleanup unless this item needs an executable change there. |
| Exact command identity already exists in the portable protocol. | `crates/nimbus-workloads/src/saga/teardown/dispatch.rs` binds the claim, confirmed revision, confirmed transition, and mode. | The compute command retains the complete durable and provider target fields. Its constructor stays private to the confirmation gate. |
| The portable reducer already owns every transition. | `crates/nimbus-workloads/src/saga/state/teardown.rs` owns decision, claim, inspection, result, retry, resource-free, and terminal transitions. | Compute builds candidates only through those methods. It does not reimplement phase order. |
| The capability registry is earned. | NNC6.5c and NNC6.5d need five distinct adapter concepts, while provision and restart already prove the injected-capability pattern. | Add five object-safe traits and one immutable exact registry. The dispatcher invokes the selected trait. NNC6.5b supplies test substitutes only. |
| The frozen path set cannot prove raw-action deletion. | Compute recovery tests and the server composition test exhaustively match the raw variants. | Add narrow test-only ownership for those existing tests. |
| The real crash proof cannot live in compute. | `EngineWorkloadSagaStore` is server-owned and server depends on compute. A compute dependency on server would cycle. | Add one server test child and its module registration. Use the real Engine store plus a durable test capability journal in distinct processes. |
| Restart settlement is an intentional later obligation. | `RestartSettlementPending` retains source and target execution evidence and blocks terminal recording. | Return a typed waiting disposition and make zero teardown-capability calls. NNC6.5g retires the exact target or late result and clears the obligation. |
| Cleanup finalization is later-owned. | `CleanupPending` retains the exact failed claim and evidence. | Return a typed cleanup disposition and make zero new teardown-capability calls. NNC8.3 remains the cleanup finalization owner. |

## Binding Design Decisions

1. **One portable reducer.** Workloads owns `WorkloadTeardownDecision`. Compute
   owns durable application, command authorization, routing, and bounded
   driving. It does not add a second portable decision enum.
2. **Direct winner only.** Only `AppliedByThisCall` for the exact claim
   transition can create `Execute`. `ConfirmedReplay` and
   `ConfirmedAfterAmbiguity` first persist `InspectionRequired` and then create
   `Inspect`. Conflict and unresolved ambiguity create no command.
3. **Correlated provider observations.** A provider observation retains command
   ID, confirmed revision, confirmed transition, and tenant-qualified key. It
   also retains generation, desired digest, attempt, epoch, exact target, and a
   closed outcome. Crossed observations fail before a result CAS.
4. **Compute invokes ports, and adapters own effects.** The dispatcher calls one
   injected trait method. This does not move the effect into compute. It keeps
   selection, invocation, and result correlation inside the sole coordinator.
   NNC6.5c and NNC6.5d add concrete implementations without reopening this
   dispatch logic.
5. **Five small capabilities.** The traits are final ingress withdrawal,
   execution drain, execution stop, network detach, and network release. Each
   has separate `execute` and `inspect` methods.
6. **Exact immutable registry.** Construction is all-or-nothing. Duplicate
   attachment, execution, or ingress registrations fail. A network provider
   cannot register as both attachment and ingress in this registry. Missing or
   crossed step/target selection fails without fallback. This preserves the
   canonical NNC4 `NetworkRoleConflict` rule instead of creating a second
   registration policy for teardown.
7. **Provider source digests are command fences, not registry keys.** Execute
   rechecks current source and network reports. Inspect remains available for
   an already-issued exact target after source or report drift. This prevents
   a durable old effect from becoming impossible to settle.
8. **No placeholder composition.** An empty registry is a valid fail-closed
   value for tests and protocol-only composition. NNC6.5b does not install it
   in workload-capable `ComputeState`.
9. **Retained work outlives one waiter.** Cancellation before runtime
   submission makes zero store and capability calls. After the retained task
   starts, cancellation detaches only that waiter. The task confirms any
   received result before it returns or becomes recoverable.
10. **Bounded execution.** One run has a hard decision limit and dispatches at
    most one unresolved inspection before it returns `Waiting`. Provider
    ambiguity cannot create a busy loop.
11. **No hidden recovery owner.** The explicit runtime drives an exact key. It
    does not enumerate tenants or start a watch. NNC6.1e2 owns startup
    enumeration.
12. **No terminal fabrication.** NNC6.5b returns typed waiting results for
    `RestartSettlementPending` and `CleanupPending`. It does not clear either
    obligation.

## Capability Contract

The registry contains three registration concepts and five capability maps:

| Registration | Exact key | Capabilities |
| --- | --- | --- |
| Ingress teardown | `NetworkProviderId` | final withdrawal |
| Execution teardown | `WorkloadExecutionProviderId` | drain and stop |
| Attachment teardown | `NetworkProviderId` | detach and release |

The registry permits only these selections:

| Teardown step | Required target | Selected capability |
| --- | --- | --- |
| `WithdrawPublication` | ingress | final withdrawal |
| `DrainExecution` | execution | execution drain |
| `StopExecution` | execution | execution stop |
| `DetachNetwork` | attachment | network detach |
| `ReleaseNetwork` | attachment | network release |

Every other step/target pair is a typed mismatch. There is no first-available
provider and no dynamic registration after construction.

## Confirmed Driver State Machine

| Durable input or result | Required behavior |
| --- | --- |
| `Quiescent` | Return settled with no CAS or capability call. |
| Resource-free candidate | Build with workloads, confirm, and continue with no capability call. |
| Terminal-record candidate | Build with workloads, confirm, and continue with no capability call. |
| New claim, direct CAS winner | Create `Execute`, select the exact capability, invoke it once, and confirm the correlated result. |
| New claim, replay or confirmed ambiguity | Persist `InspectionRequired`; create `Inspect`; never create `Execute`. |
| Recovered `DispatchPending` | Persist `InspectionRequired` before the provider read. |
| Recovered `InspectionRequired` | Create one exact `Inspect` command from durable truth. |
| Exact `NotCompleted` inspection | Persist the same attempt at the next epoch. Only the direct retry-claim CAS winner can execute. |
| Effect ambiguity | Persist `InspectionRequired`; do not retry from memory. |
| Inspection ambiguity or progress | Retain inspection state and return `Waiting`. |
| Exact success | Confirm the portable successor before another decision. |
| Definite failure | Confirm `CleanupPending` and return the typed failure state. |
| CAS conflict | Reload and rederive from durable truth. |
| Unresolved CAS ambiguity | Return `Waiting` with no command. |
| Restart settlement | Return `RestartSettlementPending` with zero teardown-capability calls. |
| Cleanup pending | Return `CleanupPending` with zero new teardown-capability calls. |

## Written Acceptance Contract

| ID | Verifiable success criterion |
| --- | --- |
| B1 | Recovery delegates every teardown phase to `WorkloadSagaRecord::decide_teardown`. The seven raw teardown action variants and raw resource-free/cleanup authority are absent. |
| B2 | The six concept-owned compute modules exist: decision, command, dispatch, driver, registry, and runtime. Each production file remains below 1,500 lines and owns one named concept. |
| B3 | Candidate building uses only workloads-owned claim, inspection, result, retry, resource-free, and terminal reducers. No phase transition is reconstructed in compute. |
| B4 | The confirmed command binds key, saga, confirmed revision and transition, issuing revision and transition, generation, desired digest, source and plan digests, attempt, epoch, target, subject, step, and mode. Its command constructor is not public. |
| B5 | Only a direct claim CAS winner receives `Execute`. Replay and confirmed ambiguity receive `Inspect` only after durable inspection state. Conflict and unresolved ambiguity receive no command. |
| B6 | Provider observations retain every callback fence. Crossed command, revision, transition, generation, digest, attempt, epoch, target, subject, step, or mode fails before result persistence and preserves durable bytes. |
| B7 | Execute accepts only success, definite failure, or ambiguity. Inspect accepts satisfied, not completed, definite failure, progress, or ambiguity. Compute creates retry evidence from the exact confirmed inspection. |
| B8 | The five object-safe `Send + Sync` capabilities provide real substitution. The dispatcher invokes exactly one selected method and contains no concrete provider effect. |
| B9 | Registry construction rejects duplicate role/provider registrations and network role conflict. Selection rejects missing ID and crossed step/target without fallback or invocation. Registry and runtime are `Send + Sync`. |
| B10 | Execute reauthenticates exact current source and provider reports before invocation. Stale evidence makes zero calls. Inspect remains routable after source or report drift for an already-issued command. |
| B11 | The driver confirms every provider result before it considers the next decision. Behavioral proof records exact withdraw, drain, stop, detach, release, and terminal order. |
| B12 | A resource-free step creates no command, capability selection, provider observation, or fabricated terminal evidence. |
| B13 | Two contenders for one claim produce one Execute call. The loser reloads or inspects. Exact replay produces no second effect. |
| B14 | Claim ambiguity performs exactly one fresh read. Exact observed claim becomes inspection-only; unchanged state waits; crossed state reports conflict. |
| B15 | Effect or result ambiguity cannot advance from memory. Recovery persists or retains exact inspection state before one provider read. |
| B16 | `NotCompleted` authorizes the same attempt at the next epoch once. Reused, skipped, stale, or crossed evidence fails without a call. |
| B17 | Cancellation before runtime submission makes zero store and capability calls. Cancellation after retained work starts detaches only the waiter; the retained task confirms a received result or leaves exact inspection-recoverable state. |
| B18 | One bounded run cannot spin on progress, ambiguity, conflicts, or inspection. The exact decision limit returns a typed error or waiting disposition. |
| B19 | `RestartSettlementPending` makes zero teardown-capability calls and remains durable for NNC6.5g. `CleanupPending` makes zero new calls and remains durable for NNC8.3. |
| B20 | A real distinct-process ten-cut matrix uses the server-owned Engine store: crash after each of five claim commits and after each external effect before its result CAS. Two parent tests each run five parameterized child-process cuts. Recovery uses the same attempt, returns Inspect first, and never duplicates the recorded effect. |
| B21 | The runtime is explicit and constructible from the existing coordinator, source authority, immutable provider reports, and immutable capability registry. It is not installed in `ComputeState` and owns no tenant scan. |
| B22 | Compute, server test proof, and static source checks pass. No Cargo manifest or dependency edge changes. `nimbus-network` keeps only the `nimbus-core` workspace edge. |
| B23 | NNCV035 recognizes the real reducer, command, and order seams without token-only product markers. Its direct result becomes exact `0 passed, 8 failed`: `service`, `definition-delete`, `compose`, `machine`, `ingress`, `tenant`, `compensation`, and `behavior`. It remains the sole aggregate red condition, and the 55 sole-diagnostic mutations remain exact. |
| B24 | Format, strict affected Clippy, warning-denied Rustdoc, proof lint with zero diagnostics, docs, site, and one candidate-frozen GPT-5.6 Sol/xhigh/fast review pass. A narrow correction review runs only after an accepted executable finding. |

## Exact Behavior Roster

### Decision and recovery

1. `teardown_recovery_delegates_to_workloads_reducer`
2. `raw_teardown_actions_are_absent_from_recovery_surface`
3. `teardown_cleanup_and_restart_settlement_are_typed_waits`
4. `resource_free_and_terminal_transitions_emit_no_command`

### Command and confirmation

5. `only_direct_claim_cas_winner_receives_execute`
6. `replay_and_confirmed_ambiguity_receive_inspect_only`
7. `unresolved_claim_ambiguity_emits_no_command`
8. `confirmed_command_binds_complete_claim_and_record_fence`
9. `crossed_teardown_command_result_preserves_durable_revision`

### Registry and dispatch

10. `registry_routes_all_five_exact_teardown_capabilities`
11. `registry_rejects_duplicate_role_provider_registration`
12. `registry_rejects_network_role_conflict`
13. `registry_reports_missing_exact_capability_without_fallback`
14. `registry_rejects_crossed_step_target_without_invocation`
15. `execute_reauthenticates_source_and_provider_reports`
16. `stale_execute_evidence_makes_zero_capability_calls`
17. `inspection_remains_available_after_source_and_report_drift`
18. `crossed_provider_observation_fails_before_result_cas`

### Driver and runtime

19. `teardown_claim_contenders_produce_one_execute_call`
20. `teardown_claim_conflict_reloads_durable_truth`
21. `teardown_claim_ambiguity_requires_one_fresh_read`
22. `recovered_pending_claim_persists_inspection_before_provider_read`
23. `ambiguous_effect_result_persists_inspection_required`
24. `not_completed_inspection_authorizes_same_attempt_next_epoch_once`
25. `teardown_result_ambiguity_requires_fresh_read_before_progress`
26. `teardown_driver_confirms_each_result_before_next_capability`
27. `teardown_driver_records_exact_five_step_order`
28. `resource_free_teardown_makes_zero_capability_calls`
29. `cancellation_before_runtime_submission_makes_zero_calls`
30. `cancellation_after_claim_detaches_only_waiter`
31. `in_progress_and_ambiguous_inspection_return_bounded_waiting`
32. `restart_settlement_and_cleanup_pending_make_zero_teardown_calls`

### Real-process recovery

33. `teardown_driver_process_crash_after_each_claim_inspects_before_retry`
34. `teardown_driver_process_crash_after_each_effect_never_reexecutes`

Each real-process parent test runs five named step cases. Together they cover
all ten crash cuts required by B20 without treating each parameter as a new
top-level roster test.

## Fail-Before Protocol

Before product implementation:

1. Prove all six production module paths and all four root API symbols are
   absent.
2. Prove the raw recovery action variants are present.
3. Add one named failing test for decision, command, registry, dispatch,
   driver, runtime, and the server process harness.
4. Run each exact test and record its compile or assertion failure. Do not
   weaken a test to make it compile.
5. Record test names, exit status, compiler diagnostic, checkpoint, and dirty
   paths in this proof and the Recovery Header before implementation.

The first fail-before tests are:

- `teardown_recovery_delegates_to_workloads_reducer`
- `only_direct_claim_cas_winner_receives_execute`
- `registry_rejects_duplicate_role_provider_registration`
- `stale_execute_evidence_makes_zero_capability_calls`
- `teardown_driver_records_exact_five_step_order`
- `cancellation_before_runtime_submission_makes_zero_calls`
- `teardown_driver_process_crash_after_each_claim_inspects_before_retry`

### Recorded Fail-Before Evidence

Checkpoint `76626d33541a126d32fb9aa694429f4a93b44292` had none of the six
module paths or these four root APIs:

- `WorkloadTeardownCapabilityRegistry`
- `WorkloadTeardownDispatcher`
- `WorkloadTeardownDriver`
- `WorkloadTeardownRuntime`

The same snapshot retained all eight raw recovery actions:
`WithdrawPublication`, `DrainWorkload`, `StopWorkload`, `DetachNetwork`,
`ReleaseNetwork`, `RecordTerminalEvidence`, `InspectCleanup`, and
`AdvanceWithoutEffect`.

Each named test then failed for its intended missing production seam:

| Test | Exact result | First failure | Output |
| --- | --- | --- | --- |
| `teardown_recovery_delegates_to_workloads_reducer` | `0/1`, 309 filtered, exit 101 | missing `decide_teardown` delegation | `/tmp/nnc65b-fail-before-decision.out` |
| `only_direct_claim_cas_winner_receives_execute` | `0/1`, 309 filtered, exit 101 | missing `ConfirmedWorkloadTeardownCommand` | `/tmp/nnc65b-fail-before-command.out` |
| `registry_rejects_duplicate_role_provider_registration` | `0/1`, 309 filtered, exit 101 | missing `WorkloadTeardownCapabilityRegistry` | `/tmp/nnc65b-fail-before-registry.out` |
| `stale_execute_evidence_makes_zero_capability_calls` | `0/1`, 309 filtered, exit 101 | missing `WorkloadTeardownDispatcher` | `/tmp/nnc65b-fail-before-dispatch.out` |
| `teardown_driver_records_exact_five_step_order` | `0/1`, 309 filtered, exit 101 | missing first `decide_teardown` step | `/tmp/nnc65b-fail-before-driver.out` |
| `cancellation_before_runtime_submission_makes_zero_calls` | `0/1`, 309 filtered, exit 101 | missing `WorkloadTeardownRuntime` | `/tmp/nnc65b-fail-before-runtime.out` |
| `teardown_driver_process_crash_after_each_claim_inspects_before_retry` | `0/1`, 634 filtered, exit 101 | missing public runtime for process recovery | `/tmp/nnc65b-fail-before-process.out` |

These are test-only source assertions. Implementation must replace them with
observable contract tests. It must not make them pass with token-only product
markers.

## Owned Paths

Product and compute tests:

- `crates/nimbus-compute/src/workload_saga/teardown_decision.rs`
- `crates/nimbus-compute/src/workload_saga/teardown_decision/`
- `crates/nimbus-compute/src/workload_saga/teardown_command.rs`
- `crates/nimbus-compute/src/workload_saga/teardown_command/`
- `crates/nimbus-compute/src/workload_saga/teardown_dispatch.rs`
- `crates/nimbus-compute/src/workload_saga/teardown_dispatch/`
- `crates/nimbus-compute/src/workload_saga/teardown_driver.rs`
- `crates/nimbus-compute/src/workload_saga/teardown_driver/`
- `crates/nimbus-compute/src/workload_saga/teardown_registry.rs`
- `crates/nimbus-compute/src/workload_saga/teardown_registry/`
- `crates/nimbus-compute/src/workload_saga/teardown_runtime.rs`
- `crates/nimbus-compute/src/workload_saga/teardown_runtime/`
- `crates/nimbus-compute/src/workload_saga/teardown_test_support.rs`
- `crates/nimbus-compute/src/workload_saga.rs`
- `crates/nimbus-compute/src/lib.rs`
- `crates/nimbus-compute/src/workload_saga/recovery.rs`
- `crates/nimbus-compute/src/workload_saga/recovery/tests.rs` (mechanical test replacement only)
- `crates/nimbus-compute/src/workload_saga/restart_runtime.rs` and its tests
  only if the frozen cancellation boundary requires a compile-preserving
  mechanical change.

Narrow server test-only handoff:

- `crates/nimbus-server/src/workload_saga_store/tests/teardown_driver_process.rs`
- `crates/nimbus-server/src/workload_saga_store/tests/mod.rs`
- `crates/nimbus-server/src/workload_saga_store/tests/composition.rs`

Verifier and control-plane evidence:

- `scripts/nimbus-network-control-plane/workload-teardown-source-contract.mjs`
- `scripts/nimbus-network-control-plane/workload-teardown-contract-fixture.mjs`
- `scripts/nimbus-network-control-plane/workload-teardown-contract.sh` only if
  the exact partial-red arithmetic needs a mechanical update.
- this proof, the canonical plan, and the plan index.

## Forbidden Paths And Effects

NNC6.5b must not edit:

- `crates/nimbus-compute/src/state.rs`
- `crates/nimbus-compute/src/workload_saga/test_support.rs`
- future `teardown_node.rs` or `teardown_sandbox.rs` adapters
- server ingress or listener production code
- node, systemd, sandbox, machine, guest, services, Compose, tenant, or caller
  production code
- Cargo manifests or workspace dependency edges
- `nimbus-network` source

The six new production modules must not contain provider effects. These effects
include sockets, Axum, Pingora, Netavark, nftables, gvproxy, and Iroh. The ban
also includes provider journals, machine commands, sandbox backends, tenant
policy, service names, forwarding, and packet effects.

## Verification Commands

Candidate closeout includes:

```text
cargo test -p nimbus-workloads --lib
cargo test -p nimbus-compute --lib
cargo test -p nimbus-server --lib workload_saga_store -- --test-threads=1
cargo clippy -p nimbus-workloads -p nimbus-compute -p nimbus-server --all-targets -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc -p nimbus-workloads -p nimbus-compute -p nimbus-server --no-deps
cargo fmt --all --check
bash scripts/nimbus-network-control-plane/workload-teardown-contract.sh --self-test
bash scripts/nimbus-network-control-plane/workload-teardown-contract.sh --check
bash scripts/verify-nimbus-network-control-plane.sh
bash scripts/check-docs.sh
bash scripts/verify-nimbus-docs-site.sh
```

The direct teardown contract remains red for later owners. Its exact NNC6.5b
target is `0 passed, 8 failed`, with only `service`, `definition-delete`,
`compose`, `machine`, `ingress`, `tenant`, `compensation`, and `behavior`
remaining. The aggregate must remain `35/36` with only NNCV035 red.

## Review Cadence

Do not run structured review during fail-before, implementation, cleanup, or
acceptance convergence. Run one full GPT-5.6 Sol/xhigh/fast review only after
B1-B24 and every listed gate are green. If that review finds an accepted defect
that changes executable code, run the affected proofs and one narrow correction
review. No review runs for docs-only closeout or ledger wording.

## Acceptance Ledger

| Checkpoint | Status | Evidence |
| --- | --- | --- |
| Read-only source audit | `green` | Three independent read-only lanes and owner inspection found the exact current/target seams, two test-only path corrections, and no product dependency blocker. Changed paths were none. |
| Written contract B1-B24 | `green` | Frozen in this proof before product edits. |
| Exact owned/forbidden paths | `green` | Frozen above. Server additions are test-only and avoid a dependency cycle. |
| Acceptance-freeze gates | `green` | NNCV035 is current exact `0/11`; self-test is `55/55`; aggregate is `35/36` with only NNCV035 red; NNCV008 and NNCV009 pass. Technical-writing lint has zero diagnostics. Docs pass `108`, and the site passes `17/17`. |
| Fail-before | `green` | The exact absence/presence census and seven named `0/1` failures are recorded above at checkpoint `76626d33541a126d32fb9aa694429f4a93b44292`. |
| Implementation | `red` | No NNC6.5b product source has changed. |
| Final behavior and process proof | `red` | Roster and ten-cut matrix remain. |
| Static, quality, docs, and review | `red` | Candidate does not exist. |
| Item commit | `red` | One reviewed evidence-backed NNC6.5b commit remains. |
