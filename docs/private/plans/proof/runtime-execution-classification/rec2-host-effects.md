# REC2 Host Operation Effect Classification Proof

Status: `done`.

REC2 made host operation effects explicit without changing the current runtime
admission behavior. Host operations are now classified at the `HostCallOperation`
enum, and the shared host-call path consults an installed execution plan before
dispatching to the host bridge.

## Implementation

- Added `HostCallOperation::runtime_effect_class()` in
  `crates/nimbus-runtime/src/host.rs`.
- Kept the classifier exhaustive over the enum. Adding a new
  `HostCallOperation` now requires the Rust match to classify it before the
  crate compiles.
- Classified current operations as:
  - HTTP route: `HttpRoute`
  - query entry, document get, query builder, and query read operations:
    `ObservableRead`
  - mutation entry and document writes: `Write`
  - action entry and service lookup: `ServiceExternal`
  - nested query/mutation/action and nested runtime entry: `NestedRuntime`
  - scheduler operations: `Scheduler`
  - runtime extension calls: `Extension`
- Added `RuntimeObservedEffectViolation` and
  `RuntimeExecutionPlan::observed_effect_violation(...)` in
  `crates/nimbus-runtime/src/execution_plan.rs`.
- Added `RuntimeInvocationExecutionPlanBinding` beside the existing host-call
  session binding in `crates/nimbus-runtime/src/runtime/bootstrap/state.rs`.
- Wired `enforce_observed_host_call_effect(...)` into both async and sync shared
  host-call paths in `crates/nimbus-runtime/src/runtime/bootstrap/ops/shared.rs`.

## Enforcement Contract

- The default binding is inactive, so REC2 does not reject existing queries that
  use observable host reads such as `DocumentGet`.
- Once REC3 installs a cooperative-eligible pure-local plan for an invocation,
  observed host effects that are unknown, observable, write, scheduler,
  service/external, nested runtime, extension, or HTTP route become typed
  `RuntimeObservedEffectViolation` values before the host bridge is called.
- Existing live host-call session validation and service-grant checks remain
  before host bridge dispatch. REC2 adds an effect guard; it does not replace
  session or tenant-principal enforcement.
- The runtime error at the V8 op boundary is derived from the typed violation
  because the op ABI still returns `JsErrorBox`.

## Verification

```text
cargo test -p nimbus-runtime runtime_execution_plan --lib -- --nocapture
```

Result:

```text
running 6 tests
test execution_plan::tests::runtime_execution_plan_requires_side_channel_posture_and_cpu_budget ... ok
test execution_plan::tests::runtime_execution_plan_fails_closed_for_unknown_or_effectful_operations ... ok
test execution_plan::tests::runtime_execution_plan_admits_web_pure_read_candidate ... ok
test execution_plan::tests::runtime_execution_plan_reports_typed_observed_effect_violations ... ok
test execution_plan::tests::runtime_execution_plan_keeps_node_full_ineligible_until_realm_proof ... ok
test execution_plan::tests::runtime_execution_plan_rejects_effectful_semantic_kind ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 1051 filtered out
```

```text
cargo test -p nimbus-runtime host_call_operations_have_exhaustive_runtime_effect_classes --lib -- --nocapture
```

Result:

```text
running 1 test
test host::tests::host_call_operations_have_exhaustive_runtime_effect_classes ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1056 filtered out
```

Expected verifier closeout after REC2: `Summary: 14 passed, 0 failed`.

## REC3 Handoff

REC3 must install `RuntimeExecutionPlan` for scheduler/admission decisions and
replace direct worker-loop scheduling checks against
`InvocationKind::is_convex_read_semantic_candidate`. The REC2 guard is already
in the shared host-call path, but it only becomes active for invocations that
carry an execution plan.
