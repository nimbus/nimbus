# Test Surface And Queue Ownership Cleanup Control Plan

Archived on 2026-04-08 after `TQ0` through `TQ7` completed. This file is kept
as historical execution record only; do not resume it as an active control
plane.

This is the canonical execution control plane for the next modularity,
readability, and idiomatic-Rust cleanup pass after the indexing, bootstrap, and
scenario-surface workstream.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `crates/nimbus-runtime/src/executor.rs`
- `crates/nimbus-runtime/src/executor/queue.rs`
- `crates/nimbus-runtime/src/metrics.rs`
- `crates/nimbus-engine/src/tests.rs`
- `crates/nimbus-engine/src/tenant.rs`
- `crates/nimbus-engine/src/service/subscriptions.rs`
- `crates/nimbus-storage/src/tests.rs`
- `crates/nimbus-test-support/src/http_api_fixture.rs`
- `crates/nimbus-server/src/tests/core_http/documents_and_commits.rs`
- `crates/nimbus-server/src/tests/convex_runtime/http_routes/demo_flow/seeded_usage/scenarios.rs`

Baseline verification status for this plan:

- the immediately preceding cleanup workstream was completed and archived as
  `docs/plans/archive/indexing-bootstrap-and-scenario-surface-cleanup-plan.md`
- this new control plane is being authored as a docs-only review-and-planning
  pass on 2026-04-06 after the prior cleanup implementation and archive
  handoff have already landed in the current worktree
- no new workspace-wide verification is claimed by this planning pass
- every `TQ*` implementation item must record its own focused verification
  before it can be marked `done`

---

## Purpose

The earlier cleanup passes removed the biggest production god files and thinned
many runtime, engine, storage, and server composition roots. The next highest
value cleanup is more specific: finish one remaining runtime worker-queue seam,
split the shared HTTP test fixture by route-family ownership, and move the
still-flat integration test roots into concept-owned surfaces.

This pass is intentionally not about breaking up large files just because they
are large. The target is clearer ownership where the current shape still makes
feature work, debugging, and regression hunting harder: the runtime worker
queue internals, the shared HTTP fixture surface, and the giant engine or
storage integration test roots that still append many unrelated concerns into
one file.

---

## Relationship To Other Plans

- Use `docs/plans/README.md` as the owning plan index.
- This plan is separate from:
  `docs/plans/v8-locker-fork-plan.md`,
  `docs/plans/warm-module-pool-plan.md`,
  `docs/plans/encryption-at-rest-plan.md`,
  `docs/plans/archive/convex-demos-compatibility-plan.md`,
  `docs/plans/wasmtime-backend-plan.md`,
  and `docs/plans/layered-admission-control-plan.md`.
- If work turns into Locker-fork feature development, warm execution design,
  Wasmtime backend work, admission-control redesign, encryption-at-rest work,
  or product-surface compatibility work, stop and move to the owning plan
  instead of stretching this cleanup plan across multiple streams.

---

## Scope

This plan covers:

- runtime worker-queue ownership inside
  `crates/nimbus-runtime/src/executor/queue.rs`
- shared HTTP fixture ownership inside
  `crates/nimbus-test-support/src/http_api_fixture.rs`
- movement of the remaining broad engine integration test root toward
  concept-owned surfaces inside `crates/nimbus-engine/src/tests.rs`
- movement of the remaining broad storage integration test root toward
  concept-owned surfaces inside `crates/nimbus-storage/src/tests.rs`
- cleanup of the remaining mixed native HTTP document/journal scenario root in
  `crates/nimbus-server/src/tests/core_http/documents_and_commits.rs`
- final naming, visibility, helper-placement, and docs cleanup that falls out
  of the new ownership map

This plan does not cover:

- new product features
- intentional route, wire, or API behavior changes unless explicitly recorded
- Locker-fork feature work or runtime backend experiments
- warm execution backend design
- admission-control redesign
- compatibility code for pre-launch behavior

---

## Cleanup Invariants

These rules are mandatory for every item in this plan.

1. Preserve externally observable behavior by default.
   Runtime queueing, affinity routing, fairness, shutdown, native HTTP route
   semantics, and broad engine or storage integration semantics stay unchanged
   unless a specific item explicitly records otherwise.

2. Keep the core architecture invariants intact.
   `nimbus-core` stays zero I/O.
   `nimbus-runtime` stays zero workspace dependencies.
   All mutations still flow through `Service::apply_mutation` or its queued
   async journal path.
   Storage atomicity stays unchanged.

3. Prefer concept-owned modules over helper piles.
   A successful split makes the owning concept easier to name, test, and debug.

4. Keep composition roots thin once ownership moves out.
   Do not rename a broad file into a facade without actually moving ownership.

5. Keep broad integration coverage intact.
   The engine, storage, and server roots can shrink, but the scenarios they
   protect still need to run somewhere obvious and maintainable.

6. Move shared test helpers only when ownership gets clearer.
   Do not create generic test utility piles that are harder to place than the
   cases they support.

7. Treat the current git worktree as baseline reality.
   Resume from the code and this plan's ledger, not from chat memory.

---

## Current Assessed State

- The runtime, engine, storage, and server production trees are materially more
  modular than they were at the start of the cleanup campaign.
- The remaining largest production roots are no longer automatically the best
  cleanup targets. `crates/nimbus-runtime/src/runtime.rs` and
  `crates/nimbus-runtime/src/executor.rs` are large, but they now mainly read
  as composition roots plus inline tests.
- The strongest remaining maintainability problem is the still-flat integration
  test ownership in `crates/nimbus-engine/src/tests.rs`, followed by the
  broad-but-smaller storage root in `crates/nimbus-storage/src/tests.rs`.
- The clearest remaining production seam is `crates/nimbus-runtime/src/executor/queue.rs`,
  which still combines worker job models, activity signaling, shutdown state,
  affinity-aware routing, and queue-controller behavior.
- The highest-value shared test helper seam is
  `crates/nimbus-test-support/src/http_api_fixture.rs`, which still groups
  debug, Convex, tenants, schedule, schema, document, journal, and native query
  helpers into one surface.

---

## Current Review Findings

1. `crates/nimbus-engine/src/tests.rs` is the largest remaining concept-mixed
   root in the repo.
   It still combines subscription/reactivity cases, schema and tenant basics,
   indexed query or pagination coverage, materialized-serving coverage,
   mutation-journal and cancellation semantics, durable-journal and
   embedded-replica consistency, policy behavior, and generated-history
   verification in one flat chronological file.

2. `crates/nimbus-storage/src/tests.rs` is smaller than the engine root but
   still mixes several distinct storage concepts.
   Store CRUD and atomicity basics, journal metadata, shadow-materializer or
   recovery behavior, async cancellation and fault injection, and usage-store
   time semantics still share one root.

3. `crates/nimbus-test-support/src/http_api_fixture.rs` remains a broad shared
   fixture surface.
   Debug/diagnostics routes, Convex runtime routes, tenant lifecycle routes,
   schedule and cron routes, schema routes, document or journal routes, and
   native query routes all live together.

4. `crates/nimbus-runtime/src/executor/queue.rs` is the clearest remaining
   production ownership seam.
   Runtime worker job models, result senders, worker-activity signaling,
   shutdown state, affinity-aware routing, and queue-controller completion
   behavior still live together.

5. `crates/nimbus-server/src/tests/core_http/documents_and_commits.rs` is the
   strongest remaining mixed server scenario root.
   Document lifecycle, journal paging and bootstrap, consistency reports,
   embedded-replica scenarios, and previously extracted helper modules still
   belong to distinct concepts but share one root.

6. `crates/nimbus-runtime/src/metrics.rs`,
   `crates/nimbus-engine/src/tenant.rs`,
   and `crates/nimbus-engine/src/service/subscriptions.rs`
   are no longer the best next targets.
   They are denser than small modules, but they now mostly read as coherent
   facades or already-extracted subsystem boundaries rather than the highest
   value cleanup seams for the next pass.

7. `crates/nimbus-server/src/tests/convex_runtime/http_routes/demo_flow/seeded_usage/scenarios.rs`
   is large but already cohesive.
   It reads as one owned adversarial scenario surface rather than a misplaced
   helper pile, so it is not the next cleanup target just because of size.

---

## Success Criteria

This plan is successful only when all of the following are true:

- runtime worker-queue ownership is easier to trace during routing, shutdown,
  or completion debugging
- the shared HTTP fixture surface is easier to navigate by route family
- the engine and storage integration test roots read as composition surfaces
  over concept-owned test modules instead of flat append-only files
- the remaining mixed native HTTP document/journal scenario root is easier to
  extend without scanning unrelated cases
- naming, visibility, and helper placement are more idiomatic and consistent
- no unintentionally observable behavior changes are introduced
- the plan can be archived cleanly once the workstream completes

---

## Feature Preservation Matrix

- Runtime worker routing, affinity reuse, load balancing, queueing, shutdown,
  and completion semantics must remain unchanged.
- Runtime timeout, cancellation, fairness, and async host-call permit
  suspend/resume semantics must remain unchanged.
- Native CRUD, query, paginated query, journal, bootstrap, diagnostics,
  scheduling, and embedded-replica route semantics must remain unchanged.
- Existing engine, storage, server, and runtime regression coverage must remain
  intact even when tests move into concept-owned surfaces.
- Generated-history and verification-harness seed semantics must remain
  unchanged.
- Shared test fixtures may move, but their request and response helpers must
  keep current behavior unless the plan explicitly records a cleanup-only
  rename or placement change.

---

## Control Plane Rules

1. This document is the durable control plane for this cleanup workstream.
2. Update this plan before or during every meaningful implementation burst.
3. Keep exactly one `TQ*` item `in_progress` at a time.
4. Do not skip forward while an earlier eligible item is still `todo`.
5. If an item spans multiple sessions, leave it `in_progress` and update its
   checkpoint instead of starting the next item.
6. Record verification in `Execution Log` before marking an item `done`.
7. If a blocker appears, record it in the ledger and execution log before
   stopping.
8. Treat the roadmap plus the git worktree as the source of execution state.

---

## Verification Contract

Every implementation item in this plan must:

1. run its focused verification before it is marked `done`
2. run `cargo fmt --all --check`
3. run `cargo check --workspace`
4. run the appropriate focused crate tests for the changed surface
5. record any environment limitation explicitly in `Execution Log`

Before archiving this plan, also run:

- `make check`
- `make test`
- `make clippy`
- `make ci` if practical

If `make ci` cannot complete because `cargo deny` or advisory-db locking is not
available in the environment, record that limitation explicitly rather than
silently skipping it.

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies | Gate Note |
| --- | --- | --- | --- | --- |
| TQ0 | `done` | reviewed the current post-indexing-bootstrap architecture and identified the next meaningful cleanup hotspots in runtime worker-queue ownership, shared HTTP fixtures, and remaining broad integration test roots | none | docs-only review and planning pass on 2026-04-06 |
| TQ1 | `done` | split `crates/nimbus-runtime/src/executor/queue.rs` by worker-queue ownership | none | landed as the `executor/queue/` module tree with stable executor-facing behavior |
| TQ2 | `done` | split `crates/nimbus-test-support/src/http_api_fixture.rs` by API-family fixture ownership | TQ1 recommended first | landed as the `http_api_fixture/` route-family module tree with stable helper behavior |
| TQ3 | `done` | split `crates/nimbus-engine/src/tests.rs` by concept-owned integration surfaces | TQ2 recommended first | landed as a root composition surface over `tests/subscriptions.rs`, `tests/queries.rs`, `tests/materialized_serving.rs`, `tests/mutation_journal.rs`, `tests/consistency.rs`, and `tests/policy.rs` |
| TQ4 | `done` | split `crates/nimbus-storage/src/tests.rs` by concept-owned storage surfaces | TQ2 recommended first | landed as a root composition surface over CRUD/journal, recovery, store basics, usage-store, async/fault, and generated-history modules |
| TQ5 | `done` | split `crates/nimbus-server/src/tests/core_http/documents_and_commits.rs` by concept ownership | TQ2 recommended first | landed as a scenario composition surface over lifecycle, journal, consistency, generated-history, and fault-helper modules |
| TQ6 | `done` | perform follow-on idiomatic-Rust, naming, and helper-placement cleanup after the new boundaries stabilize | TQ1 through TQ5 | completed as the canonical visibility/helper-placement sweep across the new split roots, with focused verification rerun green across runtime, engine, storage, server, and workspace lint/check surfaces |
| TQ7 | `done` | update docs, run the full verification sweep, and archive the completed plan cleanly | TQ1 through TQ6 | completed with green repo-wide `make check`, `make test`, and `make clippy`; `make ci` remains environment-limited by a read-only cargo advisory-db lock path |

---

## Dependency Graph

- `TQ1` is the recommended first slice because it is the strongest remaining
  production ownership seam and stands on its own.
- `TQ2` should usually follow `TQ1` because the shared HTTP fixture cleanup
  benefits the later server and integration-test moves.
- `TQ3` and `TQ4` are the highest-value test-surface moves once `TQ2` lands.
- `TQ5` should usually follow `TQ2` because it depends on a cleaner native HTTP
  fixture boundary and can stay smaller than the engine/storage roots.
- `TQ6` comes only after the production and test ownership seams settle.
- `TQ7` closes the workstream after all production and test items land.

---

## Recommended Delivery Order

1. `TQ1` — runtime worker-queue ownership
2. `TQ2` — shared HTTP fixture ownership
3. `TQ3` — engine integration test ownership
4. `TQ4` — storage integration test ownership
5. `TQ5` — native HTTP document/journal scenario ownership
6. `TQ6` — idiomatic naming, visibility, and helper-placement sweep
7. `TQ7` — docs, verification, and archive

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| TQ0 | done | start `TQ1` by mapping what still belongs together in `executor/queue.rs` versus the natural job, signal, router, and controller seams |
| TQ1 | done | start `TQ2` by mapping `http_api_fixture.rs` into debug/diagnostics, Convex, tenant, scheduling, schema, document/journal, and query-owned fixture seams |
| TQ2 | done | start `TQ3` by mapping `crates/nimbus-engine/src/tests.rs` into concept-owned integration modules that can sit on top of the cleaner route-family fixture surface |
| TQ3 | done | start `TQ4` by mapping `crates/nimbus-storage/src/tests.rs` into CRUD/journal, recovery, async/fault, and usage-store owned surfaces |
| TQ4 | done | start `TQ5` by mapping `crates/nimbus-server/src/tests/core_http/documents_and_commits.rs` into lifecycle, journal/bootstrap, and consistency/replica owned scenario modules |
| TQ5 | done | start `TQ6` by reviewing the newly split roots for leftover module-order, visibility, and helper-placement glue now that the ownership map is stable |
| TQ6 | done | start `TQ7` by running the repo-wide verification sweep, then archive the completed control plane and update the live entrypoint docs |
| TQ7 | done | workstream complete; keep this file archived as historical record only |

---

## Work Items

### TQ0. Baseline review and hotspot map

#### Outcome

- Completed during this planning pass.

### TQ1. Split `executor/queue.rs` by worker-queue ownership

#### Implementation plan

1. Separate runtime worker job/result models, worker-activity signaling,
   shutdown state, affinity-aware routing, and queue-controller completion
   behavior into clearer owned submodules.
2. Keep the executor-facing queue surface stable.
3. Preserve routing, fairness, shutdown, and completion semantics exactly.

#### Focused verification

- `cargo test -p nimbus-runtime`
- `cargo test -p nimbus-server`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- worker-queue ownership is easier to trace under routing or shutdown pressure
- `queue.rs` reads as a composition root rather than a mixed implementation pile

### TQ2. Split `http_api_fixture.rs` by API-family fixture ownership

#### Implementation plan

1. Separate debug/diagnostics helpers, Convex runtime helpers, tenant helpers,
   native scheduling helpers, schema helpers, document/journal helpers, and
   native query helpers into clearer fixture-owned modules.
2. Keep the fixture call surface stable for existing tests unless a cleanup-only
   rename is clearly better and recorded.
3. Preserve helper behavior exactly.

#### Focused verification

- `cargo test -p nimbus-server`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- shared HTTP fixture ownership is easier to navigate by route family
- later server and integration-test moves have clearer helper homes

### TQ3. Split `nimbus-engine/src/tests.rs` by concept-owned integration surfaces

#### Implementation plan

1. Move the broad engine root into concept-owned test modules such as
   subscriptions/reactivity, query/pagination/planner behavior,
   materialized-serving semantics, mutation-journal/cancellation behavior,
   durable-journal or replica consistency, and auth/policy behavior.
2. Keep broad engine integration coverage intact.
3. Move helpers only where ownership clearly improves.

#### Focused verification

- `cargo test -p nimbus-engine`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- `crates/nimbus-engine/src/tests.rs` reads as a composition root or thin
  entrypoint rather than a giant flat scenario file
- new engine integration cases have obvious homes

### TQ4. Split `nimbus-storage/src/tests.rs` by concept-owned storage surfaces

#### Implementation plan

1. Move CRUD, journal metadata, shadow-materializer/recovery, async cancellation
   or fault-injection, and usage-store coverage into clearer storage-owned test
   modules.
2. Keep generated-history ownership stable where it already has a good home.
3. Preserve storage coverage and semantics exactly.

#### Focused verification

- `cargo test -p nimbus-storage`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- `crates/nimbus-storage/src/tests.rs` reads as a thin root instead of a mixed
  concept pile
- storage regression cases are easier to extend without scanning unrelated tests

### TQ5. Split the remaining native HTTP document/journal root by concept ownership

#### Implementation plan

1. Separate document lifecycle cases, journal/bootstrap cases, and
   consistency/embedded-replica cases into clearer server-owned modules.
2. Keep already-extracted generated-history and fault helpers stable.
3. Preserve native HTTP scenario semantics exactly.

#### Focused verification

- `cargo test -p nimbus-server`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- the native HTTP document/journal root is easier to navigate
- lifecycle, journal, and consistency scenarios no longer live in one mixed file

### TQ6. Follow-on idiomatic-Rust and helper-placement sweep

#### Implementation plan

1. Tighten naming, module visibility, helper placement, and composition-root
   structure after the new boundaries stabilize.
2. Remove leftover glue that only existed because of the pre-split structure.
3. Keep behavior unchanged.

#### Focused verification

- `cargo test -p nimbus-engine`
- `cargo test -p nimbus-storage`
- `cargo test -p nimbus-server`
- `cargo test -p nimbus-runtime`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- naming, helper placement, and visibility are more canonical and consistent
- no stale split-glue remains

### TQ7. Docs and full verification closure

#### Implementation plan

1. Update `ARCHITECTURE.md` if any architecture-level ownership map changed.
2. Update `docs/plans/README.md`, `AGENTS.md`, and other entrypoint docs if
   plan ownership changes during the workstream.
3. Remove stale checkpoint text and ensure the ledger, dependency graph, and
   execution log match reality.
4. Archive the completed plan once all non-deferred work is done.

#### Focused verification

- `make check`
- `make test`
- `make clippy`
- `make ci` if practical

#### Acceptance criteria

- the docs reflect the landed ownership map
- the plan can be archived cleanly with no ledger/worktree mismatch

---

## Execution Log

| Date | Item | Outcome | Summary | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-06 | TQ0 | done | Reviewed the live post-indexing-bootstrap architecture and identified the next meaningful cleanup hotspots in runtime worker-queue ownership, shared HTTP fixture ownership, and the remaining broad engine, storage, and native-HTTP integration test roots. Authored this new active cleanup control plane and prepared it for promotion in the plans index and agent entrypoint. | docs-only review and planning pass; no new code verification claimed in this handoff | start `TQ1` by mapping `crates/nimbus-runtime/src/executor/queue.rs` into job/result, activity/shutdown, router, and controller-owned seams |
| 2026-04-08 | TQ1 | done | Split `crates/nimbus-runtime/src/executor/queue.rs` into the `executor/queue/` module tree: `job.rs` now owns worker job envelopes and result senders, `signal.rs` owns worker activity signaling, `shutdown.rs` owns executor shutdown state, `router.rs` owns affinity-aware dispatch and load tracking, and `controller.rs` owns the worker-local queue controller surface. Updated `ARCHITECTURE.md` to reflect the landed queue ownership map. | `cargo fmt --all --check`; `cargo check --workspace`; `bash scripts/cargo-isolated.sh -- test -p nimbus-runtime`; `bash scripts/cargo-isolated.sh -- test -p nimbus-server` | start `TQ2` by mapping `crates/nimbus-test-support/src/http_api_fixture.rs` into API-family fixture modules while keeping the existing test call surface stable |
| 2026-04-08 | TQ2 | done | Split `crates/nimbus-test-support/src/http_api_fixture.rs` into the `http_api_fixture/` route-family module tree: `debug.rs` owns diagnostics helpers, `convex.rs` owns Convex runtime and HTTP helpers, `tenants.rs` owns tenant lifecycle helpers, `schedule.rs` owns native scheduling and cron helpers, `schema.rs` owns schema helpers, `documents.rs` owns document and journal helpers, and `queries.rs` owns native query helpers. Updated `ARCHITECTURE.md` to reflect the landed `nimbus-test-support` ownership map. | `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo test -p nimbus-server` | start `TQ3` by mapping `crates/nimbus-engine/src/tests.rs` into concept-owned integration modules with clearer helper ownership |
| 2026-04-08 | TQ3 | done | Split the giant engine integration root into concept-owned module files under `crates/nimbus-engine/src/tests/`: subscriptions/reactivity, queries/planner, materialized serving, mutation journal/visibility, consistency/replica, and policy behavior now live in separate files while `crates/nimbus-engine/src/tests.rs` remains the composition surface for shared helpers and a small set of basic service/schema regressions. Updated `ARCHITECTURE.md` to reflect the landed engine test ownership map. | `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo test -p nimbus-engine` | start `TQ4` by mapping `crates/nimbus-storage/src/tests.rs` into concept-owned storage test modules |
| 2026-04-08 | TQ4 | done | Split the remaining storage integration root into concept-owned module files under `crates/nimbus-storage/src/tests/`: CRUD and durable-journal basics, shadow-materializer recovery, store basics, usage-store coverage, async/fault behavior, and generated-history coverage now live in separate files while `crates/nimbus-storage/src/tests.rs` keeps the shared helper fixtures and module declarations. Updated `ARCHITECTURE.md` to reflect the landed storage test ownership map. | `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo test -p nimbus-storage` | start `TQ5` by mapping `crates/nimbus-server/src/tests/core_http/documents_and_commits.rs` into concept-owned native HTTP scenario modules |
| 2026-04-08 | TQ5 | done | Split the native HTTP `documents_and_commits` root into concept-owned scenario files under `crates/nimbus-server/src/tests/core_http/documents_and_commits/`: lifecycle, journal/bootstrap, and consistency/embedded-replica cases now live beside the existing generated-history and fault-helper modules while the root file remains a small composition surface. | `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo test -p nimbus-server` | start `TQ6` by tightening leftover naming, visibility, and module-order glue across the new split roots |
| 2026-04-08 | TQ6 | done | Reviewed the newly split runtime queue, shared HTTP fixture, engine test, storage test, and native HTTP scenario roots for leftover split glue, then kept the canonical composition-root shape with tightened visibility/helper placement and rustfmt-owned module ordering instead of adding new helper piles or fighting formatter conventions. A clean standalone rerun of the full runtime suite passed after one transient parallel-run failure, and the remaining focused verification surfaces all closed green. | `cargo fmt --all --check`; `cargo check --workspace`; `cargo test -p nimbus-engine`; `bash scripts/cargo-isolated.sh -- test -p nimbus-storage`; `bash scripts/cargo-isolated.sh -- test -p nimbus-server`; `bash scripts/cargo-isolated.sh -- test -p nimbus-runtime`; `cargo clippy --workspace --all-targets -- -D warnings` | finish `TQ7` by running the repo-wide sweep, updating live plan-entry docs, and archiving the completed control plane |
| 2026-04-08 | TQ7 | done | Closed the workstream with the repo-wide verification sweep, archived this completed control plane under `docs/plans/archive/`, and removed the stale live-entry references from `docs/plans/README.md` and `AGENTS.md` so new agents will not resume the finished cleanup pass. | `make check`; `make test`; `make clippy`; `make ci` failed at `cargo deny check` because `/Users/jack/.cargo/advisory-dbs/db.lock` is read-only in this environment | workstream complete |
