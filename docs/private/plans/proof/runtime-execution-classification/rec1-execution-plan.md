# REC1 Internal Runtime Execution Plan Proof

Status: `done`.

REC1 introduced the internal typed execution-plan vocabulary without public API
changes and without changing current cooperative scheduling behavior. The
worker loop still uses the same query/paginated-query compatibility baseline,
but the helper is now named as semantic evidence:
`InvocationKind::is_convex_read_semantic_candidate`.

## Implementation

- Added `crates/nimbus-runtime/src/execution_plan.rs`.
- Added closed classifier vocabulary:
  `RuntimeEffectClass`, `RuntimeSideChannelPosture`,
  `CooperativeEligibility`, `CooperativeIneligibilityReason`,
  `RuntimeSchedulingClass`, `RuntimePoolAuthorityKey`,
  `RuntimeAdmissionOutcome`, and `RuntimeExecutionPlan`.
- Kept the module internal to `nimbus-runtime`; no workspace dependency was
  added.
- Kept `RuntimePoolAuthorityKey` separate from `RuntimeAffinityKey`. REC1 does
  not import or reuse `RuntimeAffinityKey` in the execution-plan module.
- Renamed `InvocationKind::allows_cooperative_multiplexing` to
  `InvocationKind::is_convex_read_semantic_candidate` so the method describes
  only Convex semantic kind, not pooling, placement, CPU fairness, host
  isolation, tenant quota, or scheduler admission.
- Added a scoped `#[expect(dead_code)]` to the new module because REC1 creates
  typed vocabulary before REC3 wires the plan into production scheduler
  admission.

## Classifier Contract

- WebLean plus `PureLocalRead` or `ObservableRead` plus proven side-channel
  posture is eligible. REC3 widened `ObservableRead` from the initial
  conservative REC1 posture so existing PIR4 read-safe host-call lanes remain
  eligible through the execution plan.
- Mutation/action semantic kinds are ineligible with `EffectfulKind`.
- Unknown, write, scheduler, service/external, nested runtime, extension, and
  HTTP route effects are ineligible.
- A plan that was classified as `PureLocalRead` still receives a typed observed
  effect violation if runtime observation reports `ObservableRead`; the
  metadata and observed effect must agree.
- NodeFull remains ineligible with `NodeFullUnproven` until REC/NFR proves
  Node realm semantics and side-channel posture.
- Missing side-channel posture and CPU-heavy scheduling class are ineligible.
- Admission outcome starts as `NotEvaluated`; tenant quota and host-pressure
  admission stay in the existing PIR7/admission paths until REC3.

## Verification

```text
cargo test -p nimbus-runtime runtime_execution_plan --lib -- --nocapture
```

Result:

```text
running 5 tests
test execution_plan::tests::runtime_execution_plan_rejects_effectful_semantic_kind ... ok
test execution_plan::tests::runtime_execution_plan_keeps_node_full_ineligible_until_realm_proof ... ok
test execution_plan::tests::runtime_execution_plan_requires_side_channel_posture_and_cpu_budget ... ok
test execution_plan::tests::runtime_execution_plan_admits_web_pure_read_candidate ... ok
test execution_plan::tests::runtime_execution_plan_fails_closed_for_unknown_or_effectful_operations ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 1050 filtered out
```

Expected verifier closeout after REC1: `Summary: 11 passed, 0 failed`.

## REC2 Handoff

REC2 must classify every `HostCallOperation` exhaustively and wire observed
host effects to the execution plan. Static codegen metadata may improve inputs,
but runtime host-op observation remains the enforcement boundary.
