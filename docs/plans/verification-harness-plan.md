# Verification Harness Plan

This is the canonical execution roadmap for Neovex's next verification and
reliability cycle. It takes the architecture review findings from the completed
performance and architecture work and turns them into a dedicated plan for the
testing harness that Neovex's native apps and supported Convex-compatible apps
will rely on.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/plans/performance-and-architecture-plan.md`
- `docs/convex/compatibility.md`
- `docs/research/reactive-database-research-guide.md`
- `crates/neovex-test-support/src/simulation.rs`
- `crates/neovex-storage/src/tests.rs`
- `crates/neovex-engine/src/tests.rs`
- `crates/neovex-server/tests/reactive_loop.rs`
- FoundationDB simulation docs
- TigerBeetle safety and VOPR references
- CockroachDB randomized testing and verification references
- Convex function and local deployment docs

---

## Purpose

Neovex now has strong targeted correctness coverage for the architecture cycle
that just landed:

- durable journal integrity and recovery regressions
- applied-watermark visibility guarantees
- OCC conflict detection
- planner-aware shadow materializer parity
- restart and replay regressions
- native HTTP, WebSocket, runtime, and supported Convex-path tests

That is good progress, but it is not yet the same thing as a mission-critical
verification architecture.

This plan exists to close that gap by building:

- a whole-system deterministic simulation harness
- generated workload and model-based oracle testing across multiple surfaces
- reproducible crash, restart, cancellation, and fault campaigns
- differential verification for the supported Convex surface
- online consistency verification for authoritative and derived state

---

## Relationship To Other Plans

1. `docs/plans/performance-and-architecture-plan.md` remains the canonical
   execution record for the completed architecture cycle it covered.

2. This document is the canonical execution plan for all new verification
   harness, adversarial testing, differential testing, and consistency-verifier
   work.

3. If this plan and the master performance plan overlap on verification-harness
   implementation details, this document wins for that scoped workstream.

4. When harness work changes observable semantics, update `ARCHITECTURE.md`
   and any relevant operator or compatibility docs in the same PR.

---

## Why This Needs Its Own Plan

The architecture review found that the current verification story is strong in
targeted spots but still incomplete as a system-level proof strategy:

- the shared deterministic harness is still only clock plus scripted storage
  faults
- the strongest generated parity tests are storage-local and run only a small
  seed set
- service, server, runtime, and Convex coverage are still mostly example-based
  rather than workload-generated and differentially checked
- there is no single seeded harness that drives restarts, apply lag,
  disconnects, cancellation, runtime scheduling, and derived-state checks under
  one reproducible history

That gap is now large enough, important enough, and long-running enough to
deserve its own durable execution control plane instead of being a footnote in
the prior implementation roadmap.

---

## Success Criteria

This plan is successful only when all of the following are true:

1. Mission-critical Neovex invariants are executable as seeded, reproducible
   properties rather than mostly hand-authored examples.

2. The same generated workload can be replayed across at least:
   - authoritative store behavior
   - engine `Service` behavior
   - native HTTP and WebSocket behavior
   - shadow materializer state and query behavior
   - embedded replica catch-up behavior

3. The supported Convex surface has a differential corpus that exercises the
   same behaviors against Neovex and a real Convex backend or equivalent local
   backend target.

4. Crash, restart, apply-lag, and cancellation scenarios are reproducible from
   saved seeds and minimal repro metadata.

5. Derived state can be checked against authoritative state online or via
   offline verifier tooling, with divergence surfaced deterministically and
   debuggably.

---

## Current Verified State

As of the baseline for this plan:

- `cargo test --workspace` is green
- deterministic `Clock` and `FaultInjector` seams already exist
- a reusable generated task-history oracle now replays across storage, engine,
  native HTTP, shadow materializer evaluation, and embedded replica reads
- seeded restart schedules now drive repeated journal recovery and scheduler
  recovery campaigns across storage and engine
- a shared blocking fault gate plus scenario metadata now drive the first
  transport and runtime liveness campaigns across server tests
- journaled reads and writes have targeted applied-watermark regressions
- the engine has targeted OCC regressions, including durable-but-unapplied
  conflict coverage
- restart recovery before serving async reads is covered
- the shadow materializer has rebuild, compaction, corruption, and seeded
  parity coverage
- planner-aware and schema-aware shadow-query parity exists
- server tests cover native and supported Convex transport behavior

These are the foundations this plan builds on.

---

## Gap Statement

The current system still lacks:

- a unified deterministic simulator for whole-system interleavings
- broader generated-history differential oracles beyond the current first slice
- broader crash and restart campaigns beyond the current journal and scheduler
  recovery slice
- a supported Convex differential corpus outside Neovex's own test universe
- online verification that authoritative and derived state still agree

---

## Reference Systems And What To Borrow

### FoundationDB

Borrow the principle of treating deterministic simulation as architecture, not
as a later testing add-on:

- pluggable nondeterministic boundaries
- seeded replay
- harsh failure injection
- property-oriented invariants

### TigerBeetle

Borrow the VOPR-style attitude for a mission-critical single binary:

- run real logic in a deterministic simulated environment
- separate safety-oriented and liveness-oriented campaigns
- treat crash, disk, and network-ish faults as normal test inputs
- prefer explicit state-machine invariants over broad faith in example tests

### CockroachDB

Borrow two complementary ideas:

- domain-aware randomized workload generation rather than byte fuzzing
- trust-but-verify consistency checks that compare independently computed state

### Convex

Use Convex as a semantic oracle for the supported public app model:

- query, mutation, pagination, subscription, auth, and scheduling behavior
- local deployment and self-hosted backend targets for differential coverage

Convex is not the durability oracle for Neovex internals. It is the behavioral
oracle for the supported compatibility surface.

---

## Execution Contract

Use this section as the default operating procedure for every item below.

### General rules

- Prefer generated histories plus clear invariants over adding many one-off
  example tests.
- Keep seeds, failure schedules, and repro metadata explicit and easy to rerun.
- New harness layers must compose with the existing `Clock` and
  `FaultInjector` seams instead of inventing parallel simulation systems.
- Keep the authoritative redb-backed path as the primary correctness oracle
  until a later document explicitly changes that stance.
- Do not weaken current correctness coverage while chasing broader harness
  breadth.

### Status model

- `todo`: not started
- `in_progress`: actively being implemented; keep exactly one item in this
  state during a single autonomous run unless this plan explicitly allows a
  safe batch
- `blocked`: cannot proceed until the recorded blocker is resolved
- `done`: acceptance criteria are met and verification is recorded in the
  execution log

### Recovery loop for every new session or post-compaction resume

1. Reread this plan's `Execution Log`, `Roadmap Status Ledger`, `Dependency
   Graph`, and `Recommended Delivery Order`, then inspect the current git
   worktree.
2. If any item is `in_progress`, resume it first.
3. If the worktree is dirty, reconcile the changes to an owning item before
   choosing new scope.
4. Implement exactly one roadmap item by default.
5. Add deterministic tests first.
6. Update this plan's ledger and execution log in the same change set as the
   code or docs.

### Minimum verification per implementation item

- targeted tests for the touched harness layer
- targeted tests for every surfaced invariant added or changed
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

For items that touch runtime, engine, storage, or server behavior in a
cross-cutting way, also run:

- `cargo test -p neovex-storage`
- `cargo test -p neovex-engine`
- `cargo test -p neovex-server`

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| VH1 | done | Build the deterministic whole-system harness core | none |
| VH2 | done | Add generated-history model and multi-surface oracle runner | VH1 |
| VH3 | done | Add crash, restart, and recovery campaigns | VH1 |
| VH4 | done | Add transport, cancellation, and liveness adversarial campaigns | VH1 |
| VH5 | done | Add supported Convex differential corpus | VH2 |
| VH6 | done | Add online consistency verifier and divergence tooling | VH2 |
| VH7 | done | Add CI seed corpus, nightly matrix, and repro workflow | VH2, VH3, VH4 |

---

## Dependency Graph

- `VH1` is the foundation.
- `VH2`, `VH3`, and `VH4` all depend on `VH1`.
- `VH5` depends on `VH2`.
- `VH6` depends on `VH2`.
- `VH7` depends on `VH2`, `VH3`, and `VH4`.

---

## Recommended Delivery Order

1. `VH1`
2. `VH2`
3. `VH3`
4. `VH4`
5. `VH5`
6. `VH6`
7. `VH7`

---

## Work Items

### VH1. Build the deterministic whole-system harness core

**Priority:** highest  
**Expected impact:** turns the current seam layer into an actual reusable
simulation harness rather than a set of isolated test helpers.

#### Current verified state

- `Clock` and `FaultInjector` already exist as production-owned seams
- `Service::new_with_simulation(...)` and `TenantStore::*_with_simulation(...)`
  already thread those seams through core paths
- the shared harness wrapper in `neovex-test-support` is still minimal

#### Implementation checkpoint

- Expand `DeterministicHarness` into a real scenario object instead of just a
  pair of seam handles.
- First implementation slice:
  - add explicit scenario metadata (`name`, `seed`)
  - add first-class cancellation, disconnect, and restart markers
  - add helper constructors for scripted and seeded fault schedules
  - convert one representative storage, engine, and server test to the new
    harness so the API is proven across all three layers before broader rollout

#### Implementation plan

1. Expand `neovex-test-support` with a seeded harness type that owns:
   - deterministic time
   - fault schedules
   - repro seed and scenario metadata
   - restart points
   - transport disconnect and reconnect triggers
   - cancellation triggers

2. Add a common scenario runner API that can drive:
   - direct store tests
   - engine tests
   - server integration tests

3. Keep the harness single-process first.
   The goal is reproducible interleavings and restart control, not a
   distributed cluster simulator in the first pass.

4. Ensure every scenario can emit enough metadata to rerun exactly the same
   history locally from one seed.

#### Files to change

- `crates/neovex-test-support/src/`
- `crates/neovex-storage/src/simulation.rs`
- `crates/neovex-engine/src/`
- `crates/neovex-server/src/tests/`
- `ARCHITECTURE.md`

#### Acceptance criteria

- the shared harness owns more than clock plus scripted storage faults
- harness scenarios are seed-reproducible
- restart, disconnect, and cancellation are first-class scenario inputs
- new harness APIs extend existing seam types rather than bypassing them

#### Verification

- add targeted harness unit tests in `neovex-test-support`
- convert at least one existing storage, engine, and server test each to the
  new shared harness
- run crate suites for storage, engine, and server

---

### VH2. Add a generated-history model and multi-surface oracle runner

**Priority:** highest after `VH1`  
**Expected impact:** shifts verification from isolated examples toward
replayable properties and cross-surface convergence.

#### Current verified state

- storage has seeded shadow-materializer parity tests
- engine has planner-aware shadow-query parity tests
- server has end-to-end transport tests

#### Implementation checkpoint

- First implementation slice will stay narrow and explicit:
  - add one reusable generated task-history model with logical slots
  - replay that same history across storage, engine, and native HTTP
  - use the local model as the oracle for final state, one filtered ordered
    query, and one pagination contract
  - extend the engine surface check to shadow materializer and embedded replica
    so `VH2` already covers more than one read surface above the authoritative
    store

#### Implementation plan

1. Define a workload model that can generate histories containing:
   - inserts, updates, deletes
   - indexed and non-indexed queries
   - pagination
   - subscriptions
   - scheduler operations where appropriate
   - auth or policy-context changes where supported

2. Build a replay runner that can execute the same history against:
   - `TenantStore`
   - `Service`
   - native HTTP and WebSocket surfaces
   - shadow materializer query evaluation
   - embedded replica catch-up path where applicable

3. Add explicit invariants:
   - final authoritative state matches across surfaces
   - query and pagination results converge for equivalent visibility
   - subscription outputs match equivalent query semantics
   - bootstrap plus journal replay reconstructs the same visible state

4. Keep workloads domain-aware.
   Do not use raw byte fuzzing when model-aware operations can reach much more
   useful state space.

#### Files to change

- `crates/neovex-test-support/src/`
- `crates/neovex-storage/src/tests.rs`
- `crates/neovex-engine/src/tests.rs`
- `crates/neovex-server/src/tests/`

#### Acceptance criteria

- at least one reusable history generator exists
- the same generated history can run against multiple surfaces
- failures report the seed, minimized repro metadata, and the first violated
  invariant
- existing targeted parity tests remain, but the new workload runner becomes
  the preferred place for new system-level invariants

#### Verification

- add generated-history regression tests for storage, engine, and server
- run the crate suites touched by the new runner

---

### VH3. Add crash, restart, and recovery campaigns

**Priority:** high  
**Expected impact:** upgrades recovery verification from targeted regressions to
systematic restart and replay campaigns.

#### Current verified state

- focused restart coverage exists for journal recovery before async reads
- storage already has durable journal replay and shadow recovery coverage

#### Implementation plan

1. Add harness support for scripted restart points around:
   - durable append before apply
   - checkpoint export and publish
   - compaction boundaries
   - scheduler claim and completion transitions

2. Generate recovery scenarios that prove:
   - no committed durable record is lost
   - no uncommitted write becomes visible
   - restart recovery converges to the same authoritative state
   - derived state rebuilds or fails loudly

3. Cover both single restart and repeated restart histories.

4. Record recovery-specific invariants and metrics in failure output so broken
   scenarios are debuggable without rereading the harness code.

#### Files to change

- `crates/neovex-test-support/src/`
- `crates/neovex-storage/src/tests.rs`
- `crates/neovex-engine/src/tests.rs`
- `ARCHITECTURE.md`

#### Acceptance criteria

- restart scenarios are seed-reproducible
- recovery campaigns exercise more than one boundary and more than one restart
- recovery assertions check authoritative and derived state, not just absence
  of panics

#### Verification

- add targeted recovery campaigns for journal, shadow materializer, and
  scheduler flows
- run storage and engine test suites

---

### VH4. Add transport, cancellation, and liveness adversarial campaigns

**Priority:** high  
**Expected impact:** broadens server and runtime verification beyond
example-based happy paths and isolated cancellation regressions.

#### Current verified state

- there are targeted async cancellation regressions
- reactive loop integration tests cover native and supported Convex transport
- runtime metrics and cancellation paths already have focused coverage

#### Implementation plan

1. Add adversarial scenarios for:
   - client disconnect while reads wait on applied visibility
   - disconnect after durable acknowledgment but before apply
   - reconnect and resubscribe after interruption
   - runtime queueing, cancellation, and timeout interactions

2. Add liveness-oriented scenarios inspired by TigerBeetle's distinction
   between safety and liveness:
   - after a period of injected faults, heal the core preconditions needed for
     progress
   - assert the system resumes serving, catching up, and pushing updates

3. Keep liveness assertions explicit.
   Avoid vague "did not hang" checks when progress counters or expected events
   can be asserted directly.

#### Files to change

- `crates/neovex-test-support/src/`
- `crates/neovex-engine/src/tests.rs`
- `crates/neovex-server/src/tests/`
- `crates/neovex-runtime/src/`

#### Acceptance criteria

- adversarial transport scenarios are seed-reproducible
- cancellation and liveness assertions are explicit and measurable
- server and runtime harnesses share the same scenario vocabulary where
  possible

#### Verification

- add adversarial transport and runtime scenarios
- run engine, runtime, and server test suites

---

### VH5. Add a supported Convex differential corpus

**Priority:** high  
**Expected impact:** validates Neovex's supported Convex surface against a real
external semantic oracle rather than only against internal expectations.

#### Current verified state

- Neovex has broad in-repo supported Convex tests
- compatibility docs explicitly scope support to a partial evolving subset

#### Implementation plan

1. Define a differential corpus limited to the supported subset:
   - queries
   - mutations
   - paginated queries
   - subscriptions
   - scheduling where feasible
   - supported auth shapes

2. Build runners that execute the same corpus against:
   - Neovex with the in-repo `packages/convex` client
   - a real Convex local deployment or self-hosted backend target

3. Normalize result comparison where transport-level differences are allowed
   but semantic output must match.

4. Treat unsupported Convex behaviors as out of scope and encode that scope
   explicitly in the test corpus so false failures do not blur the contract.

#### Files to change

- `crates/neovex-server/src/tests/`
- `packages/convex/`
- test-support utilities as needed
- `docs/convex/compatibility.md`

#### Acceptance criteria

- at least one real external Convex-backed differential suite exists
- the suite is limited to the documented supported subset
- failures clearly state whether Neovex diverged from the supported contract or
  the case is outside scope

#### Verification

- run the differential corpus against both targets
- keep the in-repo Convex compatibility tests green

---

### VH6. Add an online consistency verifier and divergence tooling

**Priority:** medium-high  
**Expected impact:** adds Cockroach-style trust-but-verify defense in depth for
authoritative and derived state.

#### Current verified state

- shadow materializer rebuild parity exists
- bootstrap and replay parity tests exist
- there is no runtime or operator-facing verifier that periodically compares
  independently computed state

#### Implementation plan

1. Define what can be checked online without changing the trust boundary:
   - authoritative snapshot hashes
   - shadow materializer snapshot hashes
   - embedded replica snapshot hashes
   - journal bootstrap snapshot metadata

2. Add verifier entrypoints that can:
   - run in tests
   - run as offline tooling
   - optionally run in debug or operator flows when enabled

3. On mismatch, surface:
   - the specific invariant that failed
   - the compared scopes
   - the first diff location or identifier available

4. Keep the verifier deterministic and side-effect free.

#### Files to change

- `crates/neovex-storage/src/`
- `crates/neovex-engine/src/`
- `crates/neovex-server/src/`
- `ARCHITECTURE.md`
- operator docs if new debug routes or CLI commands are added

#### Acceptance criteria

- a verifier can compare authoritative and derived state on demand
- mismatches fail loudly and diagnostically
- the verifier itself has targeted corruption and mismatch regressions

#### Verification

- add verifier-specific tests with both matching and divergent state
- run storage, engine, and server test suites

---

### VH7. Add CI seed corpus, nightly matrix, and repro workflow

**Priority:** medium  
**Expected impact:** makes the harness operationally useful rather than just
locally impressive.

#### Current verified state

- targeted and workspace test suites already run in CI-oriented workflows
- there is no structured seed corpus or nightly adversarial matrix yet

#### Implementation plan

1. Define a seed corpus with:
   - smoke seeds for normal PR runs
   - heavier adversarial seeds for nightly or scheduled runs
   - historically bug-finding seeds kept as regression fixtures

2. Add a repro workflow that lets developers rerun a failing seed and scenario
   with one command.

3. Keep the fast path fast.
   PR verification should use a focused harness slice; nightly verification can
   explore deeper state space.

4. Document how new bug-finding seeds graduate into the regression corpus.

#### Files to change

- CI config
- `crates/neovex-test-support/src/`
- `docs/README.md` or other operator/developer docs if needed

#### Acceptance criteria

- harness runs have explicit PR and nightly modes
- failing scenarios print enough metadata to rerun locally
- historically valuable seeds can be checked in as named regressions

#### Verification

- verify the repro workflow locally
- verify the chosen CI commands are deterministic for fixed seeds

---

## Execution Log

| Date | Item | Outcome | Notes | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-01 | plan | drafted | Created a dedicated follow-on verification harness plan after the architecture review concluded that the current test matrix is solid but still missing a whole-system deterministic simulator, multi-surface generated oracles, and external Convex differential coverage. | `cargo test --workspace` | begin `VH1` |
| 2026-04-01 | VH1 | in_progress | Started the first harness-core slice. The dirty worktree already contains the completed architecture-cycle fixes and docs; this item now owns the follow-on expansion of `neovex-test-support::DeterministicHarness` into a scenario-oriented harness with seed metadata plus cancellation, disconnect, and restart markers, followed by representative storage, engine, and server test conversions. | worktree reconciliation and code review | land the shared harness core, convert representative tests, and run targeted plus crate-level verification |
| 2026-04-01 | VH1 | done | Moved the shared harness core into `neovex-storage::simulation` so storage, engine, server, and `neovex-test-support` all consume the same scenario type without dependency cycles. The harness now carries explicit scenario metadata plus named cancellation, disconnect, and restart markers, supports scripted and seeded fault schedules, and exposes a generic `ServiceFixture::new_with_harness(...)` helper. Converted representative storage, engine, and server tests to the shared harness and updated architecture notes to reflect the broader deterministic seam surface. | `cargo test -p neovex-storage scenario_signal_wait_returns_after_trigger_even_if_triggered_first -- --nocapture`; `cargo test -p neovex-test-support new_with_harness_passes_scenario_context_to_the_builder -- --nocapture`; `cargo test -p neovex-storage injected_fault_before_visibility_rolls_back_the_write_deterministically -- --nocapture`; `cargo test -p neovex-engine manual_clock_advances_scheduled_work_without_wall_clock_sleep -- --nocapture`; `cargo test -p neovex-server journal_bootstrap_route_returns_snapshot_and_durable_cut -- --nocapture`; `cargo test -p neovex-storage -p neovex-test-support -p neovex-engine -p neovex-server`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | begin `VH2` |
| 2026-04-01 | VH2 | in_progress | Started the first generated-history slice. This item now owns a reusable domain-aware task-history model with logical document slots, a shared replay helper for insert/update/delete histories across different surfaces, and the first multi-surface oracle tests for final state, filtered ordered queries, and pagination. | worktree reconciliation and code review | land the reusable history model, cover storage + service/shadow/replica + native HTTP, then run targeted and crate-level verification |
| 2026-04-01 | VH2 | done | Landed the first generated-history oracle slice in `neovex-storage::simulation`: `GeneratedTaskHistory` now owns logical-slot insert/update/delete scenarios, canonical query and pagination builders, plus sync and async replay helpers. Storage, engine, and native HTTP now replay the same seeded history and compare final state, filtered ordered queries, and pagination against one local model; the engine surface also checks shadow materializer snapshots and embedded replica reads against that same oracle. | `cargo test -p neovex-storage generated_task_history_is_reproducible_for_the_same_seed -- --nocapture`; `cargo test -p neovex-storage generated_task_history_async_replay_preserves_slot_bindings -- --nocapture`; `cargo test -p neovex-storage generated_task_history_matches_model_on_storage_surface -- --nocapture`; `cargo test -p neovex-engine generated_task_history_matches_model_across_live_shadow_and_embedded_replica_surfaces -- --nocapture`; `cargo test -p neovex-server generated_task_history_matches_model_on_native_http_surface -- --nocapture`; `cargo test -p neovex-storage -p neovex-test-support -p neovex-engine -p neovex-server`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | begin `VH3` |
| 2026-04-01 | VH3 | done | Added recovery-oriented restart scheduling to `neovex-storage::simulation` via `ScriptedRestartSchedule`, `RestartBoundary`, and `RestartPoint`, then used that shared vocabulary to land the first restart campaigns. Storage now runs a seeded repeated-restart durable-journal recovery scenario that keeps unapplied durable records invisible before recovery, verifies authoritative convergence after each restart, and rebuilds shadow state from checkpoints after recovery. Engine now runs a scheduler campaign that crosses claim and completion restart boundaries without losing pending jobs or double-applying completed ones. | `cargo test -p neovex-storage scripted_restart_schedule_is_reproducible_for_the_same_seed -- --nocapture`; `cargo test -p neovex-storage generated_recovery_campaign_replays_durable_journal_across_repeated_restarts_and_rebuilds_shadow_state -- --nocapture`; `cargo test -p neovex-engine scheduler_recovery_campaign_survives_claim_and_completion_restart_boundaries -- --nocapture`; `cargo test -p neovex-storage -p neovex-engine`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | begin `VH4` |
| 2026-04-01 | VH4 | done | Added the first transport and runtime liveness campaigns on top of the shared scenario vocabulary. `neovex-test-support` now exposes a reusable `BlockingFaultInjector` so server tests can deterministically pause at a storage fault point and then heal. Native transport coverage now includes a seeded WebSocket reconnect/resubscribe scenario that drops a subscription during durable-but-unapplied lag, proves the resubscribe waits for applied visibility, then proves the healed connection catches up and keeps receiving pushes. Runtime coverage now includes a seeded queued-request-drop campaign that cancels both queued and in-flight work under isolate pressure, heals the runtime, and proves fresh work is served exactly once afterward. | `cargo test -p neovex-test-support blocking_fault_injector_waits_until_release -- --nocapture`; `cargo test -p neovex-server --test reactive_loop websocket_reconnect_and_resubscribe_catches_up_after_apply_lag_and_keeps_pushing -- --nocapture`; `cargo test -p neovex-server dropped_queued_runtime_request_recovers_and_serves_new_work_after_pressure_clears -- --nocapture`; `cargo test -p neovex-engine -p neovex-runtime -p neovex-server`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | begin `VH5` |
| 2026-04-02 | VH5 | in_progress | Started the first supported Convex differential slice in `packages/convex`. Added a shared fixture app for a stable messages workload, a differential runner that compares Neovex against the supported subset across mutation/query/paginated-query/subscription behavior, and package self-tests for the normalization and scope guardrails. The external-oracle path now resolves the official Convex browser client from an override or a nearby `convex-backend` checkout instead of a machine-specific hardcoded path. Compatibility docs now describe how to export the shared fixture app and run the Neovex-only or external comparison modes. | `npm run test --workspace convex`; `npm run test:differential --workspace convex -- --neovex-only` | run the external differential mode against a provisioned Convex deployment, then extend the corpus to scheduling and supported auth shapes |
| 2026-04-02 | VH5 | done | Completed the first supported Convex differential corpus and closed the external-oracle gaps it exposed. `packages/convex/src/differential.mjs` can now export the shared fixture, automatically start an official local Convex deployment from a nearby `convex-backend` checkout, compare Neovex and official results across named semantic slices, and report all mismatches from one run instead of only the first diff. To support the shared official-style fixture, `@neovex/codegen` now parses imported server validators such as `paginationOptsValidator`, the Convex registry bootstraps emitted schema manifests into newly created Convex tenants, and runtime query-builder `paginate(...)` now preserves the official manual-pagination continuation contract for full terminal pages. | `npm run test --workspace @neovex/codegen`; `npm run test --workspace convex`; `cargo test -p neovex-runtime runtime_query_paginate -- --nocapture`; `cargo test -p neovex-server convex_runtime_only_query_paginate_keeps_continuation_cursor_for_full_terminal_page -- --nocapture`; `cargo test -p neovex-server convex_app_schema_manifest_bootstraps_indexed_queries_for_new_tenants -- --nocapture`; `npm run test:differential --workspace convex -- --require-external`; `cargo fmt --all --check` | begin `VH6`; separately extend the Convex differential corpus to scheduling and supported auth shapes |
| 2026-04-02 | VH6 | done | Added the first online consistency-verifier slice to the engine and server. `Service::verify_consistency_async(...)` now captures one durable bootstrap cut, rebuilds an authoritative in-memory projection to that cut, compares it against a shadow-materializer replay and an embedded replica built from the same snapshot plus journal suffix, fingerprints each surface canonically, and reports deterministic pairwise mismatches with first-diff paths. The verifier also checks raw bootstrap metadata invariants separately so applied-lag snapshots are validated without being mistaken for divergence. Server debug routing now exposes the same report at `GET /debug/tenants/{tenant_id}/consistency`, and targeted regressions cover both the green live-state path and explicit snapshot/bootstrap mismatch diagnostics. | `cargo test -p neovex-engine online_consistency_verifier_matches_authoritative_shadow_and_replica_state -- --nocapture`; `cargo test -p neovex-engine snapshot_comparison_reports_document_field_differences_with_identifier -- --nocapture`; `cargo test -p neovex-engine durable_journal_bootstrap_verifier_reports_resume_after_mismatch -- --nocapture`; `cargo test -p neovex-server tenant_consistency_route_returns_green_report_for_live_state -- --nocapture` | begin `VH7`; optionally add periodic/operator automation around the on-demand verifier |
| 2026-04-02 | VH7 | done | Added an operational verification-harness layer on top of the seeded simulation work. `neovex-storage::simulation` now defines named generated-history seed corpora for explicit `pr` and `nightly` modes, including stable regression case ids plus deterministic one-command repro strings that pin `NEOVEX_VERIFY_CASE`. Storage, engine, and native HTTP now each expose ignored corpus tests for both modes, `scripts/verification-harness.sh` wraps `pr`, `nightly`, and exact-case `repro` runs, the `Makefile` exposes matching `verify-harness-*` targets, and CI now runs a focused per-surface PR matrix separately from a heavier scheduled nightly matrix. Docs now explain where the corpus lives and how historically valuable bug-finding seeds graduate into stable named regressions. | `cargo test -p neovex-storage verification_harness_seed_corpus -- --nocapture`; `bash scripts/verification-harness.sh pr storage`; `bash scripts/verification-harness.sh repro engine pr regression-two-page-pagination-41`; `cargo test -p neovex-server verification_harness_pr_generated_history_seed_corpus_matches_model -- --ignored --nocapture` (outside sandbox for local port binding); `bash scripts/verification-harness.sh nightly engine`; `make verify-harness-repro SURFACE=engine MODE=pr CASE=regression-two-page-pagination-41`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | verification-harness plan complete; next work should start from a fresh follow-on plan or a new architecture review |
