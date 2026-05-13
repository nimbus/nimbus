# Verification Architecture

This document extends [ARCHITECTURE.md](../../ARCHITECTURE.md) with the deeper
verification and harness topology that should not sit in the stable
architecture root. The root architecture doc keeps the system-level invariants;
this doc keeps the proof surfaces, harness ownership, and corpus layout that
make those invariants testable.

For the repo-wide proof discipline around semantic waits, bounded budgets,
deterministic hardship, and helper ownership, see
[reliability-posture.md](reliability-posture.md). For the operational path from
CI failure to evidence-backed fix, see
[ci-failure-investigation.md](ci-failure-investigation.md).

## Testing Layers

Unit tests live beside the owning crates. Integration tests for HTTP and
WebSocket end-to-end behavior live in `nimbus-server/tests/reactive_loop.rs`.
Shared fixtures live in `nimbus-testing`. In-memory store constructors such as
`TenantStore::create_in_memory()` and `UsageStore::create_in_memory()` keep the
fast storage lanes off disk.

The highest-value regression clusters now live closer to the concepts they
protect instead of piling up only in crate-root `tests.rs` files:

- scheduler persistence regressions sit beside `scheduler/`
- execution-unit OCC and finalization regressions sit beside
  `service/execution_units/`
- the seeded Convex demo-flow surface splits model, support, and scenario
  ownership under `demo_flow/seeded_usage/`
- engine integration tests now compose over concept-owned files such as
  `tests/subscriptions.rs`, `tests/queries.rs`,
  `tests/materialized_serving.rs`, `tests/mutation_journal.rs`,
  `tests/consistency.rs`, and `tests/policy.rs`
- storage integration tests now compose over concept-owned files such as
  `tests/crud_and_journal.rs`, `tests/recovery.rs`, `tests/store_basics.rs`,
  `tests/usage_store.rs`, `tests/async_faults.rs`, and
  `tests/generated_history.rs`

New regressions should prefer those concept-owned files over reopening one flat
root.

## Canonical CI Buckets

CI intentionally separates fast correctness, adversarial harnesses, external
conformance, and reporting-only proof surfaces. Keep new tests in the narrowest
bucket that proves the behavior without making unrelated lanes slower or
harder to diagnose:

- `Rust Format` and `Rust Clippy` prove Rust style and lint health only.
- `Rust Workspace Tests` runs the ordinary non-runtime Rust workspace suite
  through `cargo nextest`, plus explicit doctests because nextest does not run
  doctests.
- `Rust Runtime Tests` runs the runtime crate's ordinary product tests and
  lightweight manifest/unit checks while skipping the official Node
  compatibility corpus.
- `Rust Dependency Audit` is a Rust trust gate for licenses, bans, duplicate
  dependencies, and advisories. It lives beside the Rust format/lint/test
  lanes in the workflow and is included in `Rust Gate Summary`, even though it
  is security-oriented rather than a compile/test step.
- `<Surface> Verification Harness` runs the required ignored corpus for one
  deterministic surface: storage, engine, server, or runtime. The script mode
  and visible CI label both use `required` because this lane runs on pushes as
  well as pull requests.
- `Nightly <Surface> Verification Harness` runs the heavier scheduled corpus
  for the same deterministic surfaces.
- `Rust Node Compatibility Corpus` and `Node Compatibility Evidence` live in
  the separate Node compatibility workflow. They are external conformance and
  evidence lanes, not the runtime verification harness.
- `JavaScript Build and Test` runs package and demo self-tests through the npm
  workspace scripts.
- `Proof Helper Checks` and `Coverage` are specialized trust/reporting lanes.
  Coverage excludes `nimbus-runtime` so V8 and Node-compatibility behavior
  stays in the runtime, harness, and conformance lanes instead of being rerun
  under instrumentation.

The local `make test` target remains a full Rust libtest sweep for developers
who intentionally want the broad local run. `make ci` uses the same required
bucket shape as hosted CI: format, clippy, dependency audit, runtime tests,
workspace tests, doctests, required verification harness, JavaScript build/test,
and proof-helper checks. Hosted CI still owns coverage upload and the separate
scheduled/manual Node compatibility evidence workflow.

## Simulation Seams

The first concrete seam layer now lives in `nimbus-storage::simulation`.
`Clock` and `FaultInjector` are production-owned interfaces, not ad hoc test
helpers. `TenantStore::*_with_simulation(...)` and
`Service::new_with_simulation(...)` accept deterministic implementations,
storage commit visibility exposes named fault points, and engine scheduler
tests can advance time without wall-clock sleeps.

That module is now a composition root over:

- `simulation/clocks.rs`
- `faults.rs`
- `coordination.rs`
- `harness.rs`
- `generated.rs`
- `verification.rs`

Later journal, checkpoint, and compaction work should extend these seam types
instead of inventing parallel harness APIs.

The shared `DeterministicHarness` now also lives on that seam layer rather
than in a higher-level test-only island. It carries explicit scenario metadata
(`name`, `seed`), supports scripted or seeded fault schedules, and exposes
named cancellation, disconnect, and restart markers so storage, engine, and
server tests can share one reproducible scenario vocabulary.

## Runtime And Cross-Crate Harness Ownership

Runtime semantic tests no longer rely on drifting `RuntimeLimits::default()`
behavior unless the default itself is the subject under test.
`nimbus-runtime::test_support` owns named runtime test profiles,
subprocess-isolation helpers for V8-sensitive cooperative and warm-pool tests,
and stable runtime repro case metadata.

Cross-crate campaigns then share the same vocabulary through `nimbus-testing`,
which now owns:

- common eventual-assertion helpers
- CI-aware timing-budget helpers for proof surfaces that can share
  `nimbus-testing`
- `DeterministicTestCase` failure context
- reusable runtime profile helpers used by server and transport campaigns
- canonical shared fault-gate primitives used by engine and server adversarial
  tests

`nimbus-runtime` keeps the same timing-helper contract locally inside its test
support rather than depending on `nimbus-testing`, preserving the
zero-workspace-dependency invariant while keeping the reliability posture
aligned across proof surfaces.

That same simulation layer also owns the generated-history oracle slice.
`GeneratedTaskHistory` models logical-slot insert/update/delete workloads,
exposes canonical filtered query and paginated-query builders, and ships sync
plus async replay helpers so higher layers do not have to rewrite scenario
logic per surface.

The same module also carries restart scheduling via
`ScriptedRestartSchedule`, `RestartBoundary`, and `RestartPoint`. Those types
give storage and engine tests one shared way to describe restart boundaries
such as durable-append-before-apply, scheduler claim, and scheduler completion.

## Differential And Consistency Verification

`nimbus-testing` now complements the shared scenario vocabulary with reusable
`BlockingFaultInjector` and `ArmedBlockingFaultInjector` primitives for
adversarial engine and server tests. The current transport/runtime liveness
slice uses them to pause the authoritative write path after durable append but
before apply, drop and re-establish a WebSocket subscription under that lag,
and then prove the reconnected subscription both catches up and resumes
reactive pushes once the fault is released.

The first external Convex semantic oracle now lives in
`packages/convex/src/differential.mjs`. It reuses one shared messages fixture
app, can start an official local Convex deployment automatically from a nearby
`convex-backend` checkout, and compares Nimbus against the supported Convex
subset across mutations, queries, manual pagination, and subscriptions.

Nimbus now also has its first online trust-but-verify path for authoritative
and derived state. `Service::verify_consistency_async(...)` captures one
durable bootstrap cut, rebuilds an authoritative in-memory projection from the
raw materialized snapshot plus journal suffix, then compares that projection
against a shadow-materializer replay and an embedded replica built from the
same inputs. Operators can request that report through
`GET /debug/tenants/{tenant_id}/consistency`.

## Harness Corpora And Entry Points

The harness now has an operational seed corpus rather than only ad hoc
scenario constructors. `nimbus-storage::simulation` defines named generated-
history seeds for explicit `required` and `nightly` modes, and failure context
for those corpus runs prints a one-command repro that pins the exact named case
through `NIMBUS_VERIFY_CASE`.

CI runs the focused required corpus separately from the heavier scheduled
nightly corpus, and local entrypoints live in
`scripts/verification-harness.sh` plus the matching `make verify-harness-*`
targets.

That taxonomy now includes first-class surfaces for:

- `storage`
- `engine`
- `server`
- `runtime`

The server harness surface also owns a transport-liveness corpus in addition
to the generated-history replay corpus: websocket disconnect cleanup,
auth-change resubscribe semantics, scheduler history publication, and runtime
fairness rejection paths all run through the same named `required` / `nightly` /
`repro` entrypoints instead of remaining isolated to ordinary unit-test names.

Those harness corpus tests are ignored by default inside the ordinary workspace
suite and only run through the dedicated verification-harness lanes. The
harness launcher fails if a requested corpus surface matches zero tests, and
only the server harness surface currently narrows to `--test-threads=1`
because that dedicated ignored corpus still boots multiple ephemeral HTTP
fixtures that need serialized port binding.

## Runtime Compatibility Buckets

Runtime tests have three separate owners:

- Ordinary runtime tests prove Nimbus runtime behavior: bundle integrity,
  host-bridge contracts, timeout/cancellation behavior, locker lifecycle,
  cooperative scheduling, warm-pool reuse, and executor fairness.
- The runtime verification harness proves product liveness and isolation
  invariants under deterministic hardship: product-default bundle health,
  repeated integrity checks, concurrent dispatch, tenant queue limits,
  tenant fairness, cooperative parking/resume paths, warm-pool cycles, and
  locker snapshot/interleave cases.
- The Node compatibility corpus proves the embedded Node surface against
  vendored upstream Node fixtures, manifest topology, supported Node20/22/24
  lane expectations, application/tooling canaries, oracle samples, and
  published evidence artifacts.

The Node compatibility runner is deliberately split by ownership while keeping
test names stable: `node/mod.rs` owns execution, `node/behavior.rs` owns named
prelude/postlude behavior data, `node/batches.rs` plus `node/batches/` own
manifest batch data, and `node/cases/` owns the included fixture groups and
explicit watchpoints. Those case files are included at the `node_compat` module
root so checked-in manifest paths such as
`runtime::tests::node_compat::node22_default_lane_executes_manifested_core_semantics_subset`
do not churn when the files are reorganized.

Do not promote a Node-compatibility fixture into the runtime verification
harness just because it is runtime-owned. Put compatibility semantics in the
Node workflow, product runtime invariants in ordinary runtime tests or the
runtime harness, and cross-surface storage/engine/server invariants in the
surface harness that owns the behavior.
