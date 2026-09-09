# REC3 Scheduler Consumption Proof

Status: `done`.

REC3 moved cooperative scheduler admission from direct
`InvocationKind::is_convex_read_semantic_candidate` checks to
`RuntimeExecutionPlan::permits_cooperative_scheduler_admission()`.
`InvocationKind` remains an input to the classifier, but the worker loop no
longer consumes it as scheduler policy.

## Implementation

- Added `RuntimeExecutionPlan::for_invocation(...)` as the single constructor
  used by executor and V8 backend invocation paths.
- Added `RuntimeExecutionPlan::permits_cooperative_scheduler_admission()` so
  the cooperative worker loop consumes a typed plan output.
- Added `RuntimeWorkerJob.execution_plan` and carry it through queue/admission,
  worker-loop admission, direct V8 backend invocation, and unmanaged runtime
  invocation paths.
- Installed the plan into runtime op state through
  `RuntimeInvocationExecutionPlanBinding` when the V8 driver resets invocation
  state, allowing REC2's shared host-call guard to observe the same plan used
  by scheduler admission.
- Changed host-pressure admission to consume `job.execution_plan.host_work_class()`
  instead of recomputing a sibling work-class predicate from the context.
- Kept `ObservableRead` eligible only when the semantic kind, runtime profile,
  pool authority, side-channel posture, operator gate, and scheduling class are
  safe. This preserves existing PIR4 read-safe host-call lanes while still
  rejecting writes, scheduler, service/external, nested runtime, extension, HTTP
  route, and unknown effects before cooperative reuse continues.

## Scheduler Contract

- Worker-loop scheduling no longer calls
  `InvocationKind::is_convex_read_semantic_candidate`.
- `InvocationKind::is_convex_read_semantic_candidate` remains only inside the
  execution-plan classifier as semantic-kind evidence.
- WebLean query and paginated-query invocations with safe side-channel posture
  and exact pool authority can use cooperative scheduler admission.
- Mutations and actions remain direct run-to-completion.
- NodeFull read jobs remain ineligible with `NodeFullUnproven` until NFR proves
  realm semantics and side-channel posture.
- Query-shaped writes fail before `DocumentInsert` reaches the host bridge when
  a cooperative-eligible observable-read plan is installed.

## Verification

```text
cargo test -p nimbus-runtime runtime_execution_plan --lib -- --nocapture
```

Result:

```text
running 12 tests
test execution_plan::tests::runtime_execution_plan_admits_web_pure_read_candidate ... ok
test execution_plan::tests::runtime_execution_plan_admits_web_observable_read_candidate ... ok
test execution_plan::tests::runtime_execution_plan_fails_closed_for_unknown_or_effectful_operations ... ok
test execution_plan::tests::runtime_execution_plan_keeps_node_full_ineligible_until_realm_proof ... ok
test execution_plan::tests::runtime_execution_plan_rejects_effectful_semantic_kind ... ok
test execution_plan::tests::runtime_execution_plan_reports_typed_observed_effect_violations ... ok
test execution_plan::tests::runtime_execution_plan_requires_side_channel_posture_and_cpu_budget ... ok
test execution_plan::tests::runtime_execution_plan_for_invocation_keeps_node_full_read_ineligible_until_proven ... ok
test execution_plan::tests::runtime_execution_plan_for_invocation_requires_safe_side_channel_posture ... ok
test execution_plan::tests::runtime_execution_plan_for_invocation_carries_host_work_class ... ok
test execution_plan::tests::runtime_execution_plan_for_invocation_admits_web_read_jobs ... ok
test execution_plan::tests::runtime_execution_plan_for_invocation_rejects_effectful_semantic_kinds ... ok

test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 1051 filtered out
```

```text
cargo test -p nimbus-runtime cooperative_execution_model --lib -- --nocapture
```

Result:

```text
running 4 tests
test executor::tests::cooperative::cooperative_execution_model_resumes_parked_invocations_after_host_completion ... ok
test executor::tests::cooperative::cooperative_execution_model_cancels_parked_invocations_on_shutdown ... ok
test executor::tests::cooperative::cooperative_execution_model_startup_snapshot_handles_multiple_parked_runtimes ... ok
test executor::tests::cooperative::cooperative_execution_model_processes_worker_invocations ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 1059 filtered out
```

```text
cargo test -p nimbus-runtime rec3_query_write_effect_violation_rejects_before_host_dispatch --lib -- --nocapture
```

Result:

```text
running 2 tests
test runtime::tests::cooperative::rec3_query_write_effect_violation_rejects_before_host_dispatch_subprocess ... ignored, runs in a subprocess to isolate cooperative locker V8 state
test runtime::tests::cooperative::rec3_query_write_effect_violation_rejects_before_host_dispatch ... ok

test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 1063 filtered out
```

```text
cargo test -p nimbus-runtime pir4_mutations_do_not_enter_multiplexed_read_safe_scheduler --lib -- --nocapture
```

Result:

```text
running 2 tests
test runtime::tests::cooperative::pir4_mutations_do_not_enter_multiplexed_read_safe_scheduler_subprocess ... ignored, runs in a subprocess to isolate cooperative locker V8 state
test runtime::tests::cooperative::pir4_mutations_do_not_enter_multiplexed_read_safe_scheduler ... ok

test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 1063 filtered out
```

Expected verifier closeout after REC3: `Summary: 17 passed, 0 failed`.

## REC4 Handoff

REC4 must align runtime context shape and codegen metadata with the execution
plan. Generated/static metadata may improve the plan input, but the REC2/REC3
host-op observation path remains the trust boundary.
