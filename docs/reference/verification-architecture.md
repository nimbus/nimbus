# Verification Architecture

This document extends [ARCHITECTURE.md](../../ARCHITECTURE.md) with the deeper
verification and harness topology that should not sit in the stable
architecture root. The root architecture doc keeps the system-level invariants;
this doc keeps the proof surfaces, harness ownership, and corpus layout that
make those invariants testable.

## Testing Layers

Unit tests live beside the owning crates. Integration tests for HTTP and
WebSocket end-to-end behavior live in `neovex-server/tests/reactive_loop.rs`.
Shared fixtures live in `neovex-testing`. In-memory store constructors such as
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

## Simulation Seams

The first concrete seam layer now lives in `neovex-storage::simulation`.
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
`neovex-runtime::test_support` owns named runtime test profiles,
subprocess-isolation helpers for V8-sensitive cooperative and warm-pool tests,
and stable runtime repro case metadata.

Cross-crate campaigns then share the same vocabulary through `neovex-testing`,
which now owns:

- common eventual-assertion helpers
- `DeterministicTestCase` failure context
- reusable runtime profile helpers used by server and transport campaigns
- canonical shared fault-gate primitives used by engine and server adversarial
  tests

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

`neovex-testing` now complements the shared scenario vocabulary with reusable
`BlockingFaultInjector` and `ArmedBlockingFaultInjector` primitives for
adversarial engine and server tests. The current transport/runtime liveness
slice uses them to pause the authoritative write path after durable append but
before apply, drop and re-establish a WebSocket subscription under that lag,
and then prove the reconnected subscription both catches up and resumes
reactive pushes once the fault is released.

The first external Convex semantic oracle now lives in
`packages/convex/src/differential.mjs`. It reuses one shared messages fixture
app, can start an official local Convex deployment automatically from a nearby
`convex-backend` checkout, and compares Neovex against the supported Convex
subset across mutations, queries, manual pagination, and subscriptions.

Neovex now also has its first online trust-but-verify path for authoritative
and derived state. `Service::verify_consistency_async(...)` captures one
durable bootstrap cut, rebuilds an authoritative in-memory projection from the
raw materialized snapshot plus journal suffix, then compares that projection
against a shadow-materializer replay and an embedded replica built from the
same inputs. Operators can request that report through
`GET /debug/tenants/{tenant_id}/consistency`.

## Harness Corpora And Entry Points

The harness now has an operational seed corpus rather than only ad hoc
scenario constructors. `neovex-storage::simulation` defines named generated-
history seeds for explicit `pr` and `nightly` modes, and failure context for
those corpus runs prints a one-command repro that pins the exact named case
through `NEOVEX_VERIFY_CASE`.

CI runs the focused PR corpus separately from the heavier scheduled nightly
corpus, and local entrypoints live in `scripts/verification-harness.sh` plus
the matching `make verify-harness-*` targets.

That taxonomy now includes first-class surfaces for:

- `storage`
- `engine`
- `server`
- `runtime`

The server harness surface also owns a transport-liveness corpus in addition
to the generated-history replay corpus: websocket disconnect cleanup,
auth-change resubscribe semantics, scheduler history publication, and runtime
fairness rejection paths all run through the same named `pr` / `nightly` /
`repro` entrypoints instead of remaining isolated to ordinary unit-test names.

Those harness corpus tests are ignored by default inside the ordinary workspace
suite and only run through the dedicated verification-harness lanes. The
harness launcher fails if a requested corpus surface matches zero tests, and
only the server harness surface currently narrows to `--test-threads=1`
because that dedicated ignored corpus still boots multiple ephemeral HTTP
fixtures that need serialized port binding.
