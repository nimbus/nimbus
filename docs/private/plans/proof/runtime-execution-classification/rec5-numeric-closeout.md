# REC5 Numeric Validation And Closeout Proof

Status: `done`.

REC5 reused PIR's existing Criterion benchmark harness and JSONL trace
methodology. It did not introduce a second measurement format.

## Implementation Findings

- The REC classifier path stayed typed and safety-preserving: worker scheduling
  consumes `RuntimeExecutionPlan::permits_cooperative_scheduler_admission`, host
  work-class admission consumes `RuntimeExecutionPlan::host_work_class`, and no
  worker-loop scheduling path consumes `InvocationKind` directly.
- While collecting REC5 numbers, the WebStandard cooperative warm-pool lane
  exposed a real hot-path regression relative to the original PIR0 baseline.
  REC5 preserved these correctness-sensitive hot-path constraints before
  recording the final exception:
  - `RuntimeWaitUntilState` records per-invocation pending waitUntil work in
    `OpState`, and `__nimbusWaitUntil(...)` marks it through a fast op.
  - no-waitUntil invocations skip the waitUntil phase entirely.
  - `WarmPool` and `WarmContextRecycle` return paths both delegate request-state
    cleanup to PIR2's centralized `prepare_warm_runtime_for_retention` gate
    before retaining a runtime.
- Those cleanups preserve PIR4 waitUntil behavior but do not eliminate the
  cooperative warm-pool latency exception.

## Artifacts

- `docs/private/plans/proof/runtime-execution-classification/artifacts/rec5-pir0-selected-trace.jsonl`
  records selected current PIR0 lanes.
- `docs/private/plans/proof/runtime-execution-classification/artifacts/rec5-pir0-current-trace-after-waituntil-phase-gate.jsonl`
  records the final focused WebStandard cooperative warm-pool hostless lane.
- `docs/private/plans/proof/runtime-execution-classification/artifacts/rec5-pir5-retained-density-current-rss.jsonl`
  records the PIR5 retained-density WebStandard current-RSS lane.
- Diagnostic traces from the regression investigation are retained:
  `rec5-pir0-current-trace.jsonl`,
  `rec5-pir0-current-trace-after-waituntil-fastpath.jsonl`, and
  `rec5-pir0-current-trace-after-warm-reset.jsonl`.

## Benchmarks

All commands used `cargo bench -p nimbus-runtime --bench runtime_pool_modes`
with `--sample-size 10 --measurement-time 1 --warm-up-time 1`.

| Lane | Criterion result | Trace comparison |
|---|---:|---:|
| WebStandard hostless run-to-completion | 1.6854-1.7192 ms | +0.89% average execution versus `pir0-trace.jsonl`; within REC5 threshold |
| Node24 hostless run-to-completion | 9.7014-9.8702 ms | -95.39% average execution versus the old PIR0 row |
| WebStandard CPU-bound cooperative warm-pool | 913.80-941.73 us | +1063.92% average execution versus `pir0-trace.jsonl`; measured exception |
| Node22 await_1ms run-to-completion | 48.824-49.873 ms | improved versus `pir0-synthetic-trace.jsonl`'s 211.01 ms average execution row |
| WebStandard await_1ms cooperative warm-pool | 6.0794-6.2569 ms | no directly comparable checked-in PIR row because this lane used the PIR blocked-row override; Criterion local history reported +99.59% |
| WebStandard retained-density RSS | 3.7592-3.9248 ms | 1,245,184 bytes/runtime versus PIR5 baseline 1,814,528 bytes/runtime; -31.38% |
| WebStandard hostless cooperative warm-pool final focused row | 890.72-908.86 us | +4254.18% average execution versus `pir0-trace.jsonl`; measured exception |

Original PIR0 Criterion baseline for the primary cooperative warm-pool hostless
lane was:

```text
runtime_pool_modes_pir0_profile_matrix/web_standard/hostless_trivial/cooperative_locker/warm_pool
                        time:   [29.058 us 29.279 us 29.509 us]
```

Final current focused REC5 run:

```text
runtime_pool_modes_pir0_profile_matrix/web_standard/hostless_trivial/cooperative_locker/warm_pool
                        time:   [890.72 us 899.74 us 908.86 us]
```

## Safety Verification

```text
cargo test -p nimbus-runtime pir4_wait_until --lib -- --nocapture
```

Result:

```text
running 4 tests
test runtime::tests::timeout_cancellation::pir4_wait_until_drains_on_cooperative_queries ... ok
test runtime::tests::timeout_cancellation::pir4_wait_until_drains_background_work_after_response_ready ... ok
test runtime::tests::timeout_cancellation::pir4_wait_until_system_timeout_bounds_background_work ... ok
test runtime::tests::timeout_cancellation::pir4_wait_until_system_budget_is_fresh_after_response_ready ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 1063 filtered out
```

```text
cargo test -p nimbus-runtime warm_pool --lib -- --nocapture
```

Result:

```text
test result: ok. 19 passed; 0 failed; 9 ignored; 0 measured; 1039 filtered out
```

REC3/REC4 safety tests remain part of the REC verifier closeout. Effectful
query-shaped writes are still denied before host dispatch, runtime context
shape still denies unavailable query/action authority before host dispatch, and
the host-op effect guard remains the trust boundary below context shape.

## Decision

REC0 through REC5 are complete because the classifier architecture, host-effect
enforcement, context alignment, and numeric validation are all recorded and
verifiable. REC5 does not claim the cooperative warm-pool path meets the old PIR0
latency threshold.

WebStandard cooperative warm-pool path remains a measured latency exception.
This measured exception blocks using REC as performance justification for broader cooperative defaults or NodeFull cooperative admission. NFR2-NFR6 may consume REC's safety vocabulary, but any NodeFull realm-lease or broader cooperative default must first close a targeted cooperative warm-pool overhead optimization item.

## Optimization Plan

Before broadening cooperative defaults:

- add phase timing around cooperative slot start, `poll_once`, response-ready
  transition, `finish_invocation`, and queue completion;
- compare `invoke_bundle_unmanaged(Some(pool), ...)` against
  `start_cooperative_locker_runtime_slot(...)` for immediately-ready hostless
  reads;
- avoid retaining a cooperative slot when the invocation resolves synchronously
  without host awaits or waitUntil work, if the phase timing proves the slot
  scheduler round trip is the cost;
- keep the existing fail-closed execution-plan and host-op effect checks while
  optimizing the scheduling path.

## Final REC Handoff

REC is closed. The broader runtime-plan-family goal is not closed: the next
eligible work is the NFR follow-on sequence, with the cooperative warm-pool
latency exception treated as a constraint on any default-broadening decision.
