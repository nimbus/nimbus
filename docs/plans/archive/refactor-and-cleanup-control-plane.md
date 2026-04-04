# Refactor And Cleanup Control Plan

Archived on 2026-04-03 after `RC0` through `RC8D` completed. This file is a
historical record, not an active control plane. For current work, start at
`docs/plans/README.md` and use an active plan from there.

This is the canonical execution control plane for the behavior-preserving
refactor and cleanup pass across the current Neovex engine, server, and runtime
hot spots.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `crates/neovex-engine/src/service/mod.rs`
- `crates/neovex-engine/src/service/queries.rs`
- `crates/neovex-engine/src/service/mutations.rs`
- `crates/neovex-engine/src/service/subscriptions.rs`
- `crates/neovex-engine/src/service/execution_units.rs`
- `crates/neovex-engine/src/tenant.rs`
- `crates/neovex-engine/src/subscriptions.rs`
- `crates/neovex-server/src/adapters/convex/host_bridge/async_bridge/dispatch.rs`
- `crates/neovex-server/src/runtime/subscriptions.rs`
- `crates/neovex-server/src/owned_tasks.rs`
- `crates/neovex-runtime/src/host.rs`

Baseline verification for this plan:

- `cargo check --workspace` passed on 2026-04-03

---

## Purpose

Neovex's crate-level architecture is still sound, but the current codebase has
accumulated several large multi-responsibility files and cross-cutting seams
that now slow down safe iteration:

- `crates/neovex-engine/src/service/queries.rs` is 3073 lines
- `crates/neovex-engine/src/tenant.rs` is 2982 lines
- `crates/neovex-engine/src/service/mutations.rs` is 1040 lines
- `crates/neovex-engine/src/subscriptions.rs` is 632 lines
- the Convex host bridge still dispatches many runtime operations via repeated
  string matches across sync, cancellable, and async paths

This plan exists to drive a cleanup pass that:

1. preserves the current feature set and semantics
2. reduces local complexity and module sprawl
3. makes future work resumable through compaction and handoff without losing
   progress state
4. leaves a smaller set of single-purpose modules with explicit ownership and
   verification

This plan is not a feature roadmap. It is a code-organization and correctness
roadmap.

---

## Relationship To Other Plans

1. `docs/plans/encryption-at-rest-plan.md` still owns encryption-at-rest work.
2. `docs/plans/v8-locker-fork-plan.md` still owns the Locker fork and
   cooperative runtime workstream.
3. This plan owns behavior-preserving refactor and cleanup for currently landed
   engine, server, and runtime code.
4. If a change under this plan materially alters stable architecture-level
   behavior, update `ARCHITECTURE.md` in the same change set.
5. If work expands into a product feature, storage-format redesign, or a
   runtime-architecture change already owned by another plan, stop and amend the
   owning plan instead of silently stretching this one.

---

## Scope

This plan covers:

- modular decomposition of `TenantRuntime` internals
- modular decomposition of the engine query, mutation, and subscription
  hot-path files
- cleanup of subscription bootstrap and task-ownership seams
- replacement of duplicated stringly typed runtime host dispatch with a typed or
  single-source internal dispatch model
- consolidation of internal wrapper duplication once the large files are split
- documentation and verification updates needed to make the refactor durable

This plan does not cover:

- new product features
- intentional route or wire-protocol changes
- planner capability additions beyond preserving already-landed behavior,
  including composite index planning
- storage engine replacement or new storage formats
- encryption-at-rest work
- V8 Locker fork work
- broad performance rewrites that are not strictly required by the cleanup

---

## Refactor Invariants

These rules are mandatory for every item in this plan.

1. Behavior-preserving by default.
   Do not change externally observable behavior unless the specific item text
   explicitly says to and the change is recorded in the execution log.

2. No feature loss.
   A refactor item is not complete if a previously supported route, runtime
   operation, diagnostic snapshot, subscription path, or verification seam stops
   working.

3. Characterization before movement.
   If a risky behavior is not already covered by a targeted test, add the test
   before or alongside the extraction that would make regressions hard to
   diagnose.

4. Keep the core architecture invariants intact.
   Every mutation still flows through `Service::apply_mutation` or its queued
   async journal path.
   `neovex-runtime` still has zero workspace dependencies.
   Authorization still lives in core and engine, not in ad hoc handler code.
   Storage commit semantics stay unchanged.

5. Prefer extract-and-delegate over rewrite-in-place.
   Keep existing public `Service`, runtime, and server entrypoints stable while
   moving implementation under them into smaller modules.

6. Do not combine organization work with speculative optimization.
   If a possible optimization appears during cleanup, record it and defer it
   unless it is strictly required to keep behavior or tests intact.

7. Every partially completed work burst must checkpoint here before stopping.
   The next run should be able to reconstruct progress from this file and the
   worktree alone.

---

## Current Assessed State

As of the baseline for this plan:

- `cargo check --workspace` is green on 2026-04-03
- `Service` remains the engine composition root and now also owns dedicated
  background executors for engine and storage long-lived work
- `TenantRuntime` currently owns document caching, materialized read surfaces,
  query-planning metrics, subscription delivery, lifecycle control, mutation
  admission, mutation journal state, and diagnostics snapshots in one file
- `service/queries.rs` mixes read authorization, planner selection,
  materialized-surface reads, prepared execution, subscription bootstrap, and
  snapshot/bootstrap helpers in one implementation file
- `service/mutations.rs` still concentrates direct mutation handling, shared
  mutation authorization, async queued journal flow, and commit fan-out
- subscription bootstrap logic is still coupled to the query module rather than
  being owned entirely by subscription-facing code
- `HostCallRequest` is still serialized as `{ operation: String, payload:
  Value }`, and the Convex host bridge repeats large operation-dispatch match
  tables across sync, cancellable, and async entrypoints
- `OwnedTaskSet` is a useful but intentionally thin server-side task-ownership
  primitive; server and engine lifecycle work should preserve explicit shutdown
  and drain semantics rather than reintroducing detached tasks

---

## Success Criteria

This plan is successful only when all of the following are true:

1. The current feature set and stable behavior still work after the cleanup.
2. `TenantRuntime` becomes a composition root over smaller subsystem modules
   rather than the implementation home for nearly every tenant-local concern.
3. `service/queries.rs` is split into smaller modules with clear ownership for
   authorization, planning, execution, materialized-surface access, and
   subscription bootstrap.
4. `service/mutations.rs` is split into smaller modules with clear ownership for
   direct mutations, queued async journal flow, commit processing, and shared
   authorization helpers.
5. Subscription bootstrap and lifecycle ownership are explicit and tested
   end-to-end across engine and server paths.
6. Runtime host-call dispatch no longer relies on repeated string dispatch logic
   scattered across multiple entrypoints.
7. Cleanup leaves a clearer module map in docs and a complete execution log so a
   future Codex run can resume from the plan without chat history.
8. Final verification proves the refactor did not introduce regressions.

---

## Feature Preservation Matrix

Every implementation item must preserve these surfaces.

| Surface | Must stay true | Minimum item-level verification |
| --- | --- | --- |
| Native CRUD, query, paginated query, schema, scheduler, and journal routes | route semantics and durable behavior stay unchanged | targeted engine tests; `cargo test -p neovex-server` for touched HTTP paths |
| Native WebSocket subscriptions | initial bootstrap, live delivery, cleanup on disconnect, and unsubscribe behavior stay unchanged | targeted engine and server reactive tests |
| Convex runtime query, mutation, action, scheduler, and nested-call paths | host-call semantics and error mapping stay unchanged | targeted runtime and server Convex tests |
| Mutation admission, journal durability, and applied visibility | pre-commit cancellation, post-commit semantics, and ordered apply stay unchanged | targeted engine and storage tests |
| Authorization and policy-aware invalidation | read filters, mutation authorization, and policy-change cleanup stay unchanged | targeted engine and server authorization tests |
| Materialized read surface, serving snapshots, and diagnostics | serving selection and diagnostic snapshot shapes stay unchanged unless explicitly documented | targeted engine tests; server diagnostics tests when touched |
| Mutation execution unit and dependency tracking | snapshot sequencing, dependency recording, and conflict detection stay unchanged | targeted engine execution-unit tests |

Exact targeted commands used for each item must be recorded in `Execution Log`.

---

## Control Plane Rules

This document is the durable control plane for the cleanup workstream. The
source of truth is:

1. the current git worktree
2. this plan's `Roadmap Status Ledger`, `Implementation Checkpoints`, and
   `Execution Log`
3. `ARCHITECTURE.md`
4. the immediately relevant code and tests for the active item

Do not rely on prior chat transcripts as progress state.

### Status model

- `todo`: not started; eligible when hard dependencies are done
- `in_progress`: actively being implemented; keep exactly one item in this
  state per autonomous run
- `blocked`: cannot continue until the recorded blocker is resolved
- `done`: acceptance criteria are met and verification is recorded
- `deferred`: intentionally parked outside the current cleanup pass

### Recovery loop for every new session or post-compaction resume

1. Reread this section, `Roadmap Status Ledger`, `Implementation Checkpoints`,
   `Dependency Graph`, `Recommended Delivery Order`, and `Execution Log`.
2. Inspect the current git worktree before choosing new scope.
3. If any item is already `in_progress`, resume it first.
4. If the worktree is dirty, reconcile those changes to an owning item before
   starting anything new.
5. If no item is `in_progress`, pick the first eligible item in
   `Recommended Delivery Order` whose hard dependencies are `done`.
6. Add or confirm characterization coverage before large structural movement.
7. Update this plan's ledger, checkpoint, and log in the same change set as the
   code or docs.
8. If interrupted, compacted, or handing off, checkpoint before stopping.

### Dirty-worktree reconciliation rules

- A dirty worktree outranks remembered intent.
- If changed files clearly belong to one item, keep that item `in_progress`
  until it is either `done` or explicitly `blocked`.
- If changed files unexpectedly span multiple items, stop, record the conflict
  in `Execution Log`, and narrow the next slice before continuing.
- Do not treat partial refactor edits as disposable scratch state.

### Non-deviation rules

- Do not skip an existing `in_progress` item to start a later `todo` item.
- Do not mix behavior change and structural cleanup in the same item unless the
  item explicitly allows it.
- Do not mark an item `done` without recording verification.
- Do not close a work burst without writing the next concrete step into
  `Implementation Checkpoints` when anything remains partially complete.

### Required write-back after each work session

- update the item's status in `Roadmap Status Ledger`
- update or add the item's note in `Implementation Checkpoints` if the item
  remains partial
- append a row to `Execution Log` with date, item, outcome, verification, and
  next step
- update `ARCHITECTURE.md` in the same change set when the session lands an
  architecture-level behavior change promised by this plan

### Suggested autonomous prompt

```text
Historical prompt from when this plan was active:
Use docs/plans/archive/refactor-and-cleanup-control-plane.md as the control plane.
Reread Refactor Invariants, Control Plane Rules, Roadmap Status Ledger,
Implementation Checkpoints, Dependency Graph, Recommended Delivery Order,
Execution Log, and the current git worktree. If any item is in_progress,
resume it first. Reconcile dirty worktree changes to the owning item before
starting new scope. Implement exactly one item, run the required verification,
update the ledger/checkpoint/log, and continue. If blocked, record the blocker
in the plan before stopping. Do not rely on chat history as progress state.
```

---

## Verification Contract

### Minimum verification per implementation item

- targeted tests for the touched subsystem
- `cargo check --workspace`
- `cargo fmt --all --check`

### Additional verification by scope

- for engine query, mutation, subscription, or tenant refactors:
  `cargo test -p neovex-engine`
- for server ownership, HTTP, WebSocket, or Convex bridge refactors:
  `cargo test -p neovex-server`
- for runtime host-contract or runtime-dispatch refactors:
  `cargo test -p neovex-runtime`
- for storage-coupled mutation or visibility work:
  `cargo test -p neovex-storage`
- before marking any item `done`:
  `cargo clippy --workspace --all-targets -- -D warnings`

### Final verification before closing the plan

- `make check`
- `make test`
- `make clippy`

If `make ci` is practical at the end of the workstream, record that too.

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| RC0 | done | Freeze current behavior with characterization coverage and per-surface verification hooks | none |
| RC1 | done | Extract `TenantRuntime` internals into dedicated subsystem modules while keeping `TenantRuntime` as the facade | RC0 |
| RC2 | done | Split `service/queries.rs` into authorization, planner, prepared execution, materialized-read, and snapshot modules while keeping subscription bootstrap extractable | RC0, RC1 |
| RC3 | done | Split `service/mutations.rs` into smaller direct, queued, authorization, and commit-processing modules | RC0, RC1 |
| RC4 | done | Normalize subscription bootstrap and session/task ownership across engine and server paths | RC0, RC2, RC3 |
| RC5 | done | Replace duplicated stringly typed runtime host dispatch with a typed or single-source internal dispatch model | RC0 |
| RC6 | done | Consolidate duplicated sync/async/cancellable wrapper code after the major extractions land | RC2, RC3, RC5 |
| RC7 | done | Final cleanup, docs update, and full verification sweep | RC1, RC2, RC3, RC4, RC5, RC6 |
| RC8A | done | Remove commit-log compatibility aliases and promote durable-journal vocabulary as the only public history surface | RC7 |
| RC8B | done | Replace the public runtime host-operation string contract with a typed operation model | RC7 |
| RC8C | done | Narrow native subscription invalidation beyond coarse dependency tracking | RC7 |
| RC8D | done | Promote the serving-snapshot manager seam into the primary backend abstraction for future serving backends | RC7 |

---

## Dependency Graph

- `RC0` is the safety foundation for the rest of the plan.
- `RC1` should land before the biggest service-module splits because both
  query and mutation paths currently reach deeply into tenant internals.
- `RC2` depends on `RC1` because query execution now interacts with
  materialized-read and planning state that should have clearer subsystem seams
  first.
- `RC3` depends on `RC1` for the same reason on the mutation and journal side.
- `RC4` depends on the query and mutation splits because subscription bootstrap
  and lifecycle ownership currently straddle those files.
- `RC5` is independently valuable and can proceed once characterization is in
  place, but it should still preserve the existing runtime contract exactly.
- `RC6` waits until the large structural extractions land so it can remove
  duplication from the new module graph rather than from the old dumping-ground
  files.
- `RC7` is the closure pass that updates docs, removes leftover glue, and runs
  the full verification sweep.
- the former generic `RC8` bucket has been retired in favor of explicit
  post-cleanup follow-on items `RC8A` through `RC8D`

---

## Recommended Delivery Order

1. `RC0`
2. `RC1`
3. `RC2`
4. `RC3`
5. `RC4`
6. `RC5`
7. `RC6`
8. `RC7`
9. `RC8A`
10. `RC8B`
11. `RC8C`
12. `RC8D`

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| RC0 | done; mapped existing coverage across engine, server, runtime, and storage hot paths and captured reusable targeted verification hooks for the upcoming refactor items; confirmed native `/ws` disconnect and bootstrap cleanup were already covered in `crates/neovex-server/tests/reactive_loop/socket/subscriptions.rs` | start `RC1` by extracting tenant-local subsystems behind the current `TenantRuntime` facade without changing diagnostics or lifecycle behavior |
| RC1 | done; `TenantRuntime` is now a coordination-oriented facade over `tenant/document_cache.rs`, `tenant/materialized_reads.rs`, `tenant/mutation.rs`, `tenant/subscription_delivery.rs`, `tenant/query_planning.rs`, and `tenant/lifecycle.rs`; `tenant.rs` is down to 523 lines and retains the public facade, diagnostics assembly, and test-facing hooks | start `RC2` by splitting `service/queries.rs` into a `service/queries/` capability tree while preserving the current read, planner, materialized-surface, and bootstrap behavior |
| RC2 | done; `service/queries.rs` is now a 1204-line composition root over `service/queries/authorization.rs`, `planner.rs`, `prepared.rs`, `materialized.rs`, and `snapshot.rs`; public `Service` read entrypoints stayed in place while planner tests and private read helpers moved under the capability tree, and subscription bootstrap was later moved under `service/subscriptions/bootstrap.rs` in `RC4` | start `RC3` by extending the existing `service/mutations/` tree so direct wrappers, mutation authorization, direct store-apply helpers, and commit-processing fan-out stop sharing one monolithic file |
| RC3 | done; `service/mutations.rs` is now a 6-line composition root over `service/mutations/direct.rs`, `journal.rs`, `commit_processing.rs`, and `authorization.rs`; direct CRUD wrappers, direct store-apply helpers, commit fan-out, and shared authorization logic no longer live in one file, while the queued journal worker remains in its existing subtree | start `RC4` by moving subscription bootstrap ownership out of the query module and keeping explicit shutdown-and-drain task ownership tests green across engine and server runtime bridges |
| RC4 | done; subscription bootstrap ownership now lives under `service/subscriptions/bootstrap.rs`, policy revision checks moved with it, and runtime-backed websocket cleanup drains through `RuntimeSubscriptionHandle::shutdown_and_drain` instead of duplicating ownership logic in the socket layer | start `RC5` by replacing repeated Convex host-operation string dispatch with a typed internal operation model while preserving the serialized contract |
| RC5 | done; Convex runtime host calls now parse the serialized `operation` string once into a typed `ConvexHostOperation` enum in `crates/neovex-server/src/adapters/convex/host_bridge/async_bridge/dispatch.rs`, which then owns sync, cancellable, and async dispatch without repeating the operation registry across entrypoints | start `RC6` by collapsing obvious wrapper duplication in the stabilized query and mutation service modules without hiding critical semantics |
| RC6 | done; `service/mutations/direct.rs` now uses small shared helpers for immediate-vs-scheduled mutation result decoding, and `service/queries.rs` now builds full-table list queries through one helper instead of repeating the same `Query` literal across sync and async wrappers | start `RC7` by updating architecture docs to reflect the landed module trees and typed runtime host dispatch, then run the repo-wide `make` verification sweep |
| RC7 | done; `ARCHITECTURE.md` now describes `tenant.rs`, `service/queries.rs`, and `service/mutations.rs` as composition roots over their extracted module trees, documents subscription bootstrap ownership under `service/subscriptions/bootstrap.rs`, and records the typed internal Convex host-operation dispatcher; the repo-wide `make` verification sweep is green | start `RC8A` by removing the public commit-log alias and standardizing on durable-journal vocabulary across engine, server, docs, and tests |
| RC8A | done; removed the public `read_commit_log*` service alias, `/api/tenants/{tenant_id}/commits`, the commit-log-specific server DTOs and fixture helper, and the remaining public docs/test wording that still described the durable journal as a separate commit-log surface. Also removed the now-dead `commit_entries_from_durable_records` helper and hardened the reload recovery test to wait for the journal worker to return idle under full-suite load before asserting stats. | start `RC8B` by replacing the public raw-string runtime host-operation contract with a typed request model shared across runtime emitters and the server bridge |
| RC8B | done; promoted the host-operation registry into a public `neovex_runtime::HostCallOperation` enum, made `HostCallRequest.operation` typed, updated the runtime op emitters and executor/runtime tests to construct variants directly, switched the Convex server bridge to dispatch and label metrics from the shared enum, and replaced the old unknown-operation runtime bridge test with serde-level contract rejection coverage for invalid operation names | start `RC8C` by tracing how native subscription registrations can narrow their dependency sets without losing conservative correctness |
| RC8C | done; native subscriptions with `limit` now register and refresh `PaginatedWindowDependency` state from their currently delivered result window, so writes beyond the visible ordered boundary can be skipped without changing subscription bootstrap, disconnect, or monotonic-delivery semantics. `ARCHITECTURE.md` now describes native invalidation as conservative predicate-plus-window tracking rather than purely table-level invalidation. | start `RC8D` by tracing how `ServingSnapshotManager` still leans on the warmed-table publication backend and decide which ownership boundary to promote into a primary backend abstraction |
| RC8D | done; `tenant/materialized_reads.rs` now separates the in-memory `MaterializedServingBackend` from `ServingSnapshotManager`, so `TenantMaterializedReadSurface` composes a concrete backend and a tenant-scoped snapshot manager instead of treating warmed-table publication as the implicit serving abstraction. Table publication, warm-load coordination, retention, and publication stats now live with the backend; snapshot retention, waiter wakeups, and pinned reader handles stay with the manager, while current stats and full-scan serving semantics remain unchanged. | non-deferred cleanup and follow-on items are complete; keep future serving optimizations or alternative backends out of this control plane unless explicitly promoted |

---

## Work Items

### RC0. Freeze current behavior with characterization coverage and verification hooks

#### Implementation plan

1. Inventory the high-risk behavior surfaces from the `Feature Preservation Matrix`
   against existing tests.
2. Add missing targeted regressions before any large structural move.
3. Record the exact targeted verification commands for each future item in
   `Execution Log`.
4. Avoid any code-organization changes here beyond what is necessary to support
   the characterization coverage.

#### Files likely to change

- engine, server, runtime, or storage tests as needed
- this plan document if the verification matrix needs more detail

#### Acceptance criteria

- the highest-risk refactor surfaces are covered by targeted tests
- future items do not need to guess what to rerun after changing a subsystem
- no behavior changes land as part of the baseline-freezing step

### RC1. Extract `TenantRuntime` internals into dedicated subsystem modules

#### Implementation plan

1. Create a `tenant/` module tree or an equivalent set of dedicated subsystem
   files.
2. Extract the following concerns out of the monolithic `tenant.rs` file while
   preserving current field ownership and runtime behavior:
   - document cache
   - materialized read surface and serving snapshots
   - query-planning metrics
   - mutation admission gate
   - mutation journal state
   - subscription delivery queue
   - lifecycle control and operation guards
   - diagnostics snapshot assembly
3. Keep `TenantRuntime` as the tenant-local composition root and facade rather
   than deleting it.
4. Preserve current diagnostics and metric snapshot shapes unless an explicit
   follow-up item changes them.

#### Acceptance criteria

- `tenant.rs` is substantially smaller and coordination-oriented
- subsystem code has clear owners and smaller files
- tenant-local diagnostics and lifecycle behavior remain unchanged

### RC2. Split `service/queries.rs` into smaller capability modules

#### Implementation plan

1. Create a `service/queries/` module tree or equivalent decomposition.
2. Separate at least these concerns:
   - read authorization and policy-merge helpers
   - planner and plan-selection helpers
   - prepared execution helpers shared across sync and async paths
   - materialized-read surface selection and execution
   - subscription bootstrap evaluation and cancellation
   - snapshot/bootstrap helper logic
3. Keep the public `Service` read entrypoints stable while moving internals
   behind them.
4. Preserve already-landed planner behavior, including composite-index planning
   and query-plan metrics.

#### Acceptance criteria

- `service/queries.rs` is no longer a multi-thousand-line dumping ground
- planner and authorization logic are independently readable and testable
- materialized-surface and bootstrap logic are no longer entangled with every
  read helper

### RC3. Split `service/mutations.rs` into smaller direct, queued, and commit-processing modules

#### Implementation plan

1. Extend the existing `service/mutations/` tree rather than creating a second
   competing organization.
2. Separate at least these concerns:
   - direct insert/update/delete entrypoints and thin wrappers
   - shared mutation authorization helpers
   - queued async admission and journal worker path
   - commit processing, candidate/deleted document derivation, and subscription
     fan-out handoff
3. Preserve the single mutation path and current durable/apply/cancellation
   semantics.
4. Preserve mutation admission, journal, and delivery metrics.

#### Acceptance criteria

- `service/mutations.rs` becomes a composition layer rather than the full
  implementation home of direct and queued mutation logic
- direct, scheduled, and async queued paths still share one durable behavior
  contract
- commit processing behavior is unchanged under characterization tests

### RC4. Normalize subscription bootstrap and session/task ownership

#### Implementation plan

1. Move subscription bootstrap concerns under subscription-owned modules instead
   of leaving them in the general query dumping ground.
2. Keep explicit cleanup ownership across native WebSocket, runtime-backed
   subscription bridges, and engine subscription lifecycle code.
3. Improve module ownership first; only widen primitives like `OwnedTaskSet` if
   current tests prove they are insufficient.
4. Preserve disconnect, unsubscribe, auth-change, and cleanup semantics.

#### Acceptance criteria

- subscription bootstrap logic is easier to locate and reason about
- task ownership remains explicit and tested
- no detached child-task regressions are introduced

### RC5. Replace duplicated stringly typed runtime host dispatch with a typed or single-source internal dispatch model

#### Implementation plan

1. Keep the external serialized `operation` string contract unless an explicit
   compatibility change is approved later.
2. Parse or map that string immediately into a typed representation or a single
   internal operation registry.
3. Remove duplicated operation lists across sync, cancellable, and async
   dispatch paths.
4. Preserve current contract errors and host-call behavior under tests.

#### Acceptance criteria

- runtime host dispatch no longer depends on repeated large string match tables
- the operation set has one internal source of truth
- runtime and server tests prove behavior did not change

### RC6. Consolidate duplicated sync/async/cancellable wrapper code

#### Implementation plan

1. After the main structural splits are stable, identify duplicated wrapper
   layers across reads, writes, and subscriptions.
2. Introduce smaller shared prepared-operation helpers or wrapper-generation
   patterns where they materially reduce drift risk.
3. Keep public method names and behavior stable.
4. Remove dead glue left behind by the earlier extractions.

#### Acceptance criteria

- the remaining service wrapper code is thinner and less repetitive
- behavior is still pinned by targeted tests
- no new abstraction hides critical semantics

---

## RC0 Targeted Verification Hooks

Use these as the first focused reruns for future items before crate-wide or
workspace-wide verification. Update this list if later refactors materially move
or rename the owning tests.

- `RC1` tenant subsystem extraction:
  `cargo test -p neovex-engine service_reload_recovers_durable_journal_before_serving_async_reads`;
  `cargo test -p neovex-engine shadow_materializer_queries_match_live_service_path`;
  `cargo test -p neovex-server tenant_engine_metrics_route_surfaces_worker_and_serving_health_after_mixed_traffic`
- `RC2` query-module split:
  `cargo test -p neovex-engine query_uses_three_field_composite_range_index_through_planner`;
  `cargo test -p neovex-engine query_planning_stats_distinguish_composite_single_field_and_fallback_paths`;
  `cargo test -p neovex-engine service_read_policy_filters_full_scans_pagination_and_subscription_results`;
  `cargo test -p neovex-engine async_subscription_bootstrap_catches_up_writes_committed_before_activation`;
  `cargo test -p neovex-engine sync_subscription_bootstrap_does_not_miss_lagged_applied_commit`
- `RC3` mutation-module split:
  `cargo test -p neovex-engine mutation_async_cancellable_before_commit_rolls_back_document_index_and_durable_journal`;
  `cargo test -p neovex-engine mutation_async_cancellable_after_commit_returns_committed_result`;
  `cargo test -p neovex-engine mutation_journal_returns_only_after_apply_visibility`;
  `cargo test -p neovex-engine mutation_admission_gate_buffers_while_journal_is_paused_without_losing_in_flight_response`;
  `cargo test -p neovex-engine subscription_updates_publish_only_after_journal_apply`
- `RC4` subscription bootstrap and task ownership:
  `cargo test -p neovex-server websocket_disconnect_drops_subscription_without_explicit_unsubscribe`;
  `cargo test -p neovex-server websocket_disconnect_before_bootstrap_activation_cancels_pending_subscription_and_reconnects_cleanly`;
  `cargo test -p neovex-server websocket_unsubscribe_during_bootstrap_activation_keeps_subscription_gone`;
  `cargo test -p neovex-server convex_websocket_disconnect_releases_runtime_subscription_children`;
  `cargo test -p neovex-server convex_websocket_auth_change_drops_active_subscriptions_until_resubscribed`
- `RC5` runtime host-dispatch cleanup:
  `cargo test -p neovex-server runtime_host_request_rejects_unknown_operation_names_during_deserialization`;
  `cargo test -p neovex-server runtime_cancellable_query_builder_start_short_circuits_before_dispatch`;
  `cargo test -p neovex-server runtime_async_db_get_precancel_records_canceled_host_op_metric`;
  `cargo test -p neovex-server convex_named_query_can_use_ctx_query_host_binding`;
  `cargo test -p neovex-runtime`
- `RC6` wrapper consolidation:
  rerun the relevant `RC2`, `RC3`, and `RC5` focused commands for every touched
  surface before the owning crate suites

### RC7. Final cleanup, docs update, and full verification sweep

#### Implementation plan

1. Update `ARCHITECTURE.md` if the landed module layout or control-plane
   ownership changed materially.
2. Update `docs/plans/README.md` and any other doc indexes affected by the new
   structure.
3. Remove any temporary forwarding code or transitional notes that are no
   longer needed.
4. Run the full verification contract and record it.

#### Acceptance criteria

- docs reflect the landed structure
- all roadmap items are reconciled to the actual worktree
- final verification is recorded and green

### RC8A. Remove commit-log compatibility aliases

#### Implementation plan

1. Delete `read_commit_log*` compatibility wrappers from the engine public
   service surface in favor of durable-journal naming.
2. Remove `/api/tenants/{tenant_id}/commits` plus the commit-log-specific HTTP
   request/response DTOs from the server surface.
3. Update tests, docs, and internal callers to use the authoritative durable
   journal route and vocabulary.
4. Preserve the durable history semantics; this is a naming and surface cleanup
   rather than a change to journal ordering or visibility.

#### Acceptance criteria

- no public route or public service API still exposes commit-log vocabulary as
  a separate alias over the durable journal
- journal ordering, cursors, bootstrap, and visibility behavior stay unchanged
- docs and tests describe only the durable-journal surface

### RC8B. Replace the public runtime host-operation string contract

#### Implementation plan

1. Introduce a typed public host-operation representation at the runtime
   boundary instead of `HostCallRequest { operation: String, payload }`.
2. Update runtime op emitters, the `HostBridge` trait boundary, and the server
   Convex bridge to traffic in the typed operation model.
3. Keep serialization details internal where possible; do not preserve raw
   string operation names as the primary public API.
4. Update runtime and server tests together so contract failures stay precise.

#### Acceptance criteria

- the public runtime host boundary is typed rather than raw-string based
- operation coverage remains one source of truth across runtime and server
- behavior and error mapping remain unchanged apart from the intentional public
  contract break

### RC8C. Narrow native subscription invalidation

#### Implementation plan

1. Replace or refine the current coarse `DependencySet::from_engine_query`
   invalidation inputs for native subscriptions.
2. Preserve conservative correctness first; only reduce wakeups where the new
   dependency model proves they are unnecessary.
3. Keep bootstrap, disconnect, unsubscribe, auth-change, and monotonic-delivery
   semantics intact.
4. Add focused engine and reactive-loop coverage for the narrowed wakeup model.

#### Acceptance criteria

- native subscriptions skip more unrelated writes than they do today
- no subscription misses a required reevaluation
- delivery ordering, catch-up, and auth invalidation remain correct

### RC8D. Promote the serving-snapshot manager seam

#### Implementation plan

1. Keep the now-real `ServingSnapshot` and `ServingSnapshotManager` seam as the
   stable reader-facing abstraction.
2. Promote backend ownership so the manager no longer conceptually depends on
   the warmed-table backend as the only serious serving implementation path.
3. Prefer a dedicated serving-materializer or equivalent backend slice rather
   than growing the in-memory warmed-table backend ad hoc.
4. Preserve current read visibility and pinned-snapshot semantics while adding
   the stronger backend abstraction.

#### Acceptance criteria

- the serving-snapshot abstraction is clearly primary over any one backend
- later serving backends can plug into the same contract without widening read
  semantics
- current full-scan query, pagination, and warmed-get behavior remain correct

---

## Execution Log

Append new rows at the top of this table. Keep entries short and factual so a
future run can reconstruct progress without chat history.

| Date | Item | Outcome | Summary | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-03 | RC8D | done | Promoted the serving-snapshot seam by extracting an explicit in-memory `MaterializedServingBackend` inside `tenant/materialized_reads.rs` and making `TenantMaterializedReadSurface` compose that backend with `ServingSnapshotManager`. The backend now owns warmed-table publication, warm-load coordination, retained table versions, and publication stats, while the manager continues to own tenant-scoped serving snapshot retention, waiter wakeups, and pinned reader handles. `ARCHITECTURE.md` now records that the remaining gap is alternative backend maturity, not manager/backend ownership. | `cargo fmt --all`; `cargo test -p neovex-engine pinned_materialized_serving_snapshot_is_exact_across_multiple_loaded_tables`; `cargo test -p neovex-engine concurrent_first_load_only_publishes_caught_up_newest_materialized_table`; `cargo test -p neovex-engine`; `cargo test -p neovex-server convex_runtime_only_full_scan_query_warms_and_reuses_materialized_serving_snapshot`; `cargo test -p neovex-server convex_runtime_only_full_scan_paginated_query_reuses_materialized_serving_snapshot`; `cargo test -p neovex-server`; `cargo check --workspace`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `make check`; `make test`; `make clippy` | cleanup control-plane workstream is complete; leave future serving backend work to a newly promoted plan item rather than reopening `RC8D` implicitly |
| 2026-04-03 | RC8C | done | Narrowed native subscription invalidation for limited queries by recording `PaginatedWindowDependency` state from the currently delivered result set and refreshing that dependency window after each successful reevaluation. Native subscriptions now skip writes that land beyond the visible ordered limit window while still reacting when the window actually changes, and `ARCHITECTURE.md` now reflects the predicate-plus-window invalidation model instead of calling native tracking purely table-level. | `cargo test -p neovex-engine service_limited_subscriptions_skip_out_of_window_ordered_writes`; `cargo test -p neovex-engine`; `cargo test -p neovex-server`; `cargo check --workspace`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | start `RC8D` by promoting the serving-snapshot manager/backend seam beyond the warmed-table publication backend |
| 2026-04-03 | RC8B | done | Replaced the public raw-string runtime host-operation contract with a typed `HostCallOperation` enum in `neovex-runtime`. `HostCallRequest.operation` is now typed, runtime emitters and tests construct enum variants directly, the Convex server bridge dispatches and records metrics from the shared enum instead of reparsing strings, and invalid operation names are now rejected at serde deserialization rather than by late bridge parsing. | `cargo test -p neovex-runtime`; `cargo test -p neovex-server`; `cargo check --workspace`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | start `RC8C` by mapping native subscription dependency registration and commit-time invalidation seams against the existing runtime-backed narrower tracking model |
| 2026-04-03 | RC8A | done | Removed the public commit-log compatibility surface end to end. The engine no longer exposes `read_commit_log*`, the server no longer serves `/api/tenants/{tenant_id}/commits` or commit-log DTOs, engine tests now read durable journal records through a shared helper, current docs only describe the durable-journal surface, and the reload recovery test now waits for the journal worker to return idle before asserting final stats. | `cargo test -p neovex-engine durable_journal_`; `cargo test -p neovex-engine mutation_async_cancellable_before_commit_rolls_back_document_index_and_durable_journal`; `cargo test -p neovex-engine`; `cargo test -p neovex-server journal_`; `cargo test -p neovex-server`; `cargo check --workspace`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; initial isolated `bash scripts/cargo-isolated.sh -- test -p neovex-server ...` attempts hit a sandbox-blocked `rusty_v8` download, so the successful server verification used the shared workspace target dir instead | start `RC8B` by typing the public runtime host-operation boundary in `neovex-runtime` and the Convex server bridge |
| 2026-04-03 | RC8 | promoted | Verified the post-cleanup RC8 candidates against the landed code and replaced the generic bucket with explicit follow-on items `RC8A` through `RC8D`. Confirmed four real paths: remove durable-journal compatibility aliases, type the public runtime host-operation contract, narrow native subscription invalidation, and promote the serving-snapshot manager/backend seam. | source review of `docs/plans/refactor-and-cleanup-control-plane.md`; `ARCHITECTURE.md`; `crates/neovex-engine/src/tenant/materialized_reads.rs`; `crates/neovex-engine/src/service/queries.rs`; `crates/neovex-runtime/src/host.rs`; `crates/neovex-engine/src/subscriptions.rs`; `docs/reference/http-api.md`; `crates/neovex-server/src/router.rs`; `crates/neovex-server/src/protocol.rs` | begin `RC8A` by deleting the public commit-log alias and standardizing on durable-journal vocabulary |
| 2026-04-03 | RC7 | done | Closed the cleanup pass by aligning `ARCHITECTURE.md` with the landed module structure and running the repo-wide verification contract. The docs now describe `tenant.rs`, `service/queries.rs`, and `service/mutations.rs` as composition roots over their extracted subsystems, record subscription bootstrap ownership under `service/subscriptions/bootstrap.rs`, and note the typed internal Convex host-operation dispatcher. | `cargo test -p neovex-engine`; `cargo check --workspace`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `make check`; `make test`; `make clippy` | leave `RC8` deferred unless this plan explicitly promotes one post-cleanup candidate with scoped verification |
| 2026-04-03 | RC7 | in_progress | Started the closure pass after RC6 cleared engine, workspace, and clippy verification. Audited `ARCHITECTURE.md` and confirmed it still describes the old flat-file ownership for `TenantRuntime`, the service query and mutation paths, and the Convex host bridge, so the remaining work is documentation alignment plus the repo-wide `make` sweep. | source review of `ARCHITECTURE.md`; `docs/plans/README.md`; `cargo test -p neovex-engine`; `cargo check --workspace`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | update the architecture docs to reflect the landed module map and typed host dispatch, then run `make check`, `make test`, and `make clippy` |
| 2026-04-03 | RC6 | done | Trimmed the remaining obvious wrapper duplication without hiding semantics. `service/mutations/direct.rs` now centralizes immediate-vs-scheduled mutation result decoding for sync and async wrappers, and `service/queries.rs` now routes full-table `list_documents*` calls through one helper instead of repeating identical `Query` construction across sync and async entrypoints. | `bash scripts/cargo-isolated.sh -- test -p neovex-engine query_uses_three_field_composite_range_index_through_planner`; `bash scripts/cargo-isolated.sh -- test -p neovex-engine query_planning_stats_distinguish_composite_single_field_and_fallback_paths`; `bash scripts/cargo-isolated.sh -- test -p neovex-engine service_read_policy_filters_full_scans_pagination_and_subscription_results`; `bash scripts/cargo-isolated.sh -- test -p neovex-engine async_subscription_bootstrap_catches_up_writes_committed_before_activation`; `bash scripts/cargo-isolated.sh -- test -p neovex-engine sync_subscription_bootstrap_does_not_miss_lagged_applied_commit`; `bash scripts/cargo-isolated.sh -- test -p neovex-engine mutation_async_cancellable_before_commit_rolls_back_document_index_and_commit_log`; `bash scripts/cargo-isolated.sh -- test -p neovex-engine mutation_async_cancellable_after_commit_returns_committed_result`; `bash scripts/cargo-isolated.sh -- test -p neovex-engine mutation_journal_returns_only_after_apply_visibility`; `bash scripts/cargo-isolated.sh -- test -p neovex-engine mutation_admission_gate_buffers_while_journal_is_paused_without_losing_in_flight_response`; `bash scripts/cargo-isolated.sh -- test -p neovex-engine subscription_updates_publish_only_after_journal_apply`; `cargo test -p neovex-engine`; `cargo check --workspace`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | start `RC7` by aligning `ARCHITECTURE.md` with the landed module structure and typed host dispatch, then finish the repo-wide verification sweep |
| 2026-04-03 | RC6 | in_progress | Reconciled the control plane to the post-RC4 and post-RC5 worktree, then mapped the first safe duplication seams for cleanup. Confirmed the RC5 typed host-dispatch model is landed in the server bridge and the remaining high-value RC6 repetition is in mutation wrapper result decoding and full-table query construction for `list_documents*`. | source review of `docs/plans/refactor-and-cleanup-control-plane.md`; `crates/neovex-engine/src/service/mutations/direct.rs`; `crates/neovex-engine/src/service/queries.rs`; `crates/neovex-server/src/adapters/convex/host_bridge/async_bridge/dispatch.rs` | add small shared helpers in the stabilized engine service modules, then rerun the relevant RC2 and RC3 focused verification hooks |
| 2026-04-03 | RC5 | done | Replaced the repeated Convex host-operation string match tables with a typed parse-once internal dispatch model. `dispatch.rs` now maps the serialized `operation` string into `ConvexHostOperation`, and that enum owns the sync, cancellable, and async dispatch paths while preserving the external `HostCallRequest { operation, payload }` contract. | `cargo test -p neovex-server runtime_cancellable_unknown_operation_is_rejected`; `cargo test -p neovex-server runtime_cancellable_query_builder_start_short_circuits_before_dispatch`; `cargo test -p neovex-server runtime_async_db_get_precancel_records_canceled_host_op_metric`; `cargo test -p neovex-server convex_named_query_can_use_ctx_query_host_binding`; `cargo test -p neovex-runtime`; `cargo test -p neovex-server`; `cargo check --workspace`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | start `RC6` by trimming wrapper duplication in the stabilized service modules without introducing a heavy abstraction layer |
| 2026-04-03 | RC4 | done | Moved subscription bootstrap ownership out of the query capability tree into `service/subscriptions/bootstrap.rs` and normalized runtime-backed websocket cleanup around explicit handle shutdown. The engine bootstrap path now owns its own policy-revision and snapshot/bootstrap helpers, while the server socket layer delegates runtime subscription cleanup to `RuntimeSubscriptionHandle::shutdown_and_drain` so disconnect, unsubscribe, and auth-change semantics keep one ownership path. | `cargo test -p neovex-server websocket_disconnect_drops_subscription_without_explicit_unsubscribe`; `cargo test -p neovex-server websocket_disconnect_before_bootstrap_activation_cancels_pending_subscription_and_reconnects_cleanly`; `cargo test -p neovex-server websocket_unsubscribe_during_bootstrap_activation_keeps_subscription_gone`; `cargo test -p neovex-server convex_websocket_disconnect_releases_runtime_subscription_children`; `cargo test -p neovex-server convex_websocket_auth_change_drops_active_subscriptions_until_resubscribed`; `cargo test -p neovex-engine`; `cargo test -p neovex-server`; `cargo check --workspace`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | start `RC5` by replacing the repeated host-operation string dispatch tables with a single typed internal operation model |
| 2026-04-03 | RC4 | in_progress | Started mapping subscription bootstrap and task-ownership seams after the mutation split. Confirmed engine subscription registration still reaches into `queries` for bootstrap evaluation and policy revision, while runtime-backed subscription forwarding and cleanup are coordinated separately in the server runtime bridge with `OwnedTaskSet`. | source review of `crates/neovex-engine/src/service/subscriptions.rs`; `crates/neovex-engine/src/subscriptions.rs`; `crates/neovex-server/src/runtime/subscriptions.rs`; `crates/neovex-server/src/owned_tasks.rs` | move bootstrap evaluation under subscription-owned modules and keep the existing disconnect, unsubscribe, and runtime bridge cleanup tests green |
| 2026-04-03 | RC3 | done | Split the mutation path into dedicated authorization, direct-apply, commit-processing, and queued-journal modules while preserving the single service mutation contract. `service/mutations.rs` is now a 6-line composition root, `journal.rs` keeps the queued worker flow, `direct.rs` owns the direct/scheduled wrappers and store-apply helpers, `commit_processing.rs` owns commit fan-out, and `authorization.rs` owns shared mutation policy checks. | `cargo test -p neovex-engine mutation_async_cancellable_before_commit_rolls_back_document_index_and_commit_log`; `cargo test -p neovex-engine mutation_async_cancellable_after_commit_returns_committed_result`; `cargo test -p neovex-engine mutation_journal_returns_only_after_apply_visibility`; `cargo test -p neovex-engine mutation_admission_gate_buffers_while_journal_is_paused_without_losing_in_flight_response`; `cargo test -p neovex-engine subscription_updates_publish_only_after_journal_apply`; `cargo test -p neovex-engine`; `cargo check --workspace`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | start `RC4` by pulling subscription bootstrap ownership out of the query module and reconciling runtime bridge task ownership around the existing `OwnedTaskSet` lifecycle |
| 2026-04-03 | RC3 | in_progress | Started decomposing `service/mutations.rs` after landing the query split. Confirmed the queued async journal worker already lives in `service/mutations/journal.rs`, while the remaining monolith now mainly contains direct CRUD wrappers, direct store-apply helpers, commit-processing fan-out, and shared mutation-authorization logic. | source review of `crates/neovex-engine/src/service/mutations.rs` and `crates/neovex-engine/src/service/mutations/journal.rs` | extend the `service/mutations/` module tree so direct wrappers, commit-processing helpers, and authorization logic move out of the root file while preserving the single mutation path |
| 2026-04-03 | RC2 | done | Split `service/queries.rs` into a dedicated capability tree under `service/queries/` without changing the public read surface. Authorization, planning, prepared execution, materialized-surface reads, subscription bootstrap, and snapshot helpers now live in focused modules, and `service/queries.rs` is down from 3073 lines to 1204 lines. | `cargo test -p neovex-engine query_uses_three_field_composite_range_index_through_planner`; `cargo test -p neovex-engine query_planning_stats_distinguish_composite_single_field_and_fallback_paths`; `cargo test -p neovex-engine service_read_policy_filters_full_scans_pagination_and_subscription_results`; `cargo test -p neovex-engine async_subscription_bootstrap_catches_up_writes_committed_before_activation`; `cargo test -p neovex-engine sync_subscription_bootstrap_does_not_miss_lagged_applied_commit`; `cargo test -p neovex-engine`; `cargo check --workspace`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | start `RC3` by extending `service/mutations/` around the existing `journal.rs` module so direct wrappers, authorization, and commit processing are no longer concentrated in `service/mutations.rs` |
| 2026-04-03 | RC2 | in_progress | Started decomposing `service/queries.rs` after finishing the tenant extraction. Confirmed the current file already has natural seams between public `Service` read wrappers, planner/load helpers, materialized-read execution helpers, read-authorization/policy handling, and subscription bootstrap evaluation. | source review of `crates/neovex-engine/src/service/queries.rs` and current service module layout | extract a `service/queries/` module tree and move the private helper families behind the existing public `Service` read surface |
| 2026-04-03 | RC1 | done | Extracted `TenantRuntime` internals into dedicated subsystem modules while preserving the existing facade and diagnostics shape. `tenant.rs` now coordinates subsystem modules for document caching, materialized reads, mutation admission/journal state, subscription delivery, query planning, and lifecycle instead of housing the full implementation directly. | `cargo test -p neovex-engine service_reload_recovers_durable_journal_before_serving_async_reads`; `cargo test -p neovex-engine shadow_materializer_queries_match_live_service_path`; `cargo test -p neovex-server tenant_engine_metrics_route_surfaces_worker_and_serving_health_after_mixed_traffic`; `cargo test -p neovex-engine`; `cargo check --workspace`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | start `RC2` and split `service/queries.rs` into planner, authorization, prepared execution, materialized-read, and bootstrap modules without changing read behavior |
| 2026-04-03 | RC0 | done | Completed the characterization inventory and captured reusable targeted verification hooks for `RC1` through `RC6`. Corrected the initial websocket inventory miss after reviewing `crates/neovex-server/tests/reactive_loop/socket/subscriptions.rs`, which already covered native `/ws` disconnect cleanup, disconnect-before-bootstrap cancellation, and unsubscribe-during-bootstrap semantics. | `cargo test -p neovex-server`; `cargo check --workspace`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | start `RC1` and extract tenant-local subsystems behind the existing `TenantRuntime` facade while keeping diagnostics, lifecycle, and serving snapshot behavior unchanged |
| 2026-04-03 | RC0 | in_progress | Mapped the feature-preservation surfaces to the current engine, server, runtime, and storage tests. Confirmed broad existing coverage for journal visibility, policy-aware invalidation, materialized serving, execution-unit conflicts, HTTP routes, Convex runtime flows, and runtime-backed websocket cleanup; flagged native generic `/ws` cleanup for additional confirmation before structural movement. | document review of current test inventory across `crates/neovex-engine`, `crates/neovex-server`, `crates/neovex-runtime`, and `crates/neovex-storage` | confirm the native websocket cleanup inventory against the reactive-loop integration suite, then finish the RC0 verification contract |
| 2026-04-03 | meta | documented | Updated the repo control-plane entrypoints so future agents resume this cleanup work from `AGENTS.md`, `docs/plans/README.md`, and this plan rather than chat memory. Tightened the plan using the completed execution-ownership hardening plan as a style reference by adding the stricter recovery loop, required write-back checklist, and a suggested autonomous resume prompt. | document updates in `AGENTS.md`; `docs/plans/README.md`; review against `docs/plans/archive/execution-ownership-hardening-plan.md` | continue `RC0` from the current worktree and add the first missing characterization test before structural refactors |
| 2026-04-03 | meta | documented | Created the canonical cleanup control plane after a fresh architecture review. Reconfirmed the current workspace compile baseline (`cargo check --workspace`) and captured the current hotspot sizes and refactor order so future Codex runs can resume through compaction without relying on prior chat state. | `cargo check --workspace`; document review against current engine, server, runtime, and plans docs | start `RC0` by mapping high-risk behavior surfaces to targeted tests and adding any missing characterization coverage before structural extraction begins |
