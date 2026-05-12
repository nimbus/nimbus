# Modularity And Idiomatic Rust Cleanup Control Plan

Archived on 2026-04-03 after `MC0` through `MC7` completed. This file is a
historical record, not an active control plane. For current work, start at
`docs/plans/README.md` and use an active plan from there.

This is the canonical execution control plane for the current runtime and
engine modularity, grouped-concept ownership, and idiomatic Rust cleanup
workstream.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `crates/nimbus-engine/src/service/mod.rs`
- `crates/nimbus-engine/src/service/queries.rs`
- `crates/nimbus-engine/src/subscriptions.rs`
- `crates/nimbus-runtime/src/lib.rs`
- `crates/nimbus-runtime/src/runtime.rs`
- `crates/nimbus-runtime/src/executor.rs`
- `crates/nimbus-runtime/src/worker_loop.rs`

Baseline verification status for this plan:

- this control plane was authored and reformatted as a docs-only pass on
  2026-04-03
- no new code verification was rerun as part of that docs-only authoring step
- the first implementation item must record its own focused verification and
  the required workspace checks before it can be marked `done`

---

## Purpose

Nimbus already has a much clearer macro-architecture than it did earlier in the
project: the engine read path is split behind a query capability tree, tenant
state is organized around `TenantRuntime` as a facade and composition root, and
runtime host dispatch has a typed internal operation registry. The next cleanup
pass should build on that current architecture by making the remaining dense
runtime and engine hotspots more concept-owned, more canonical, and more
idiomatic to maintain.

This is not a lines-of-code reduction exercise. The goal is to make ownership
obvious, keep behavior stable, and leave the codebase in a shape where future
changes naturally land beside the concept they belong to.

This is not a feature roadmap. It is a code-organization, maintainability, and
correctness roadmap.

---

## Relationship To Other Plans

This plan owns the current modularity and idiomatic Rust cleanup workstream for
runtime and engine hotspots. If a change turns into encryption-at-rest work,
Locker fork work, Convex compatibility work, or admission-control design, stop
and move to the owning plan listed in `docs/plans/README.md`.

---

## Scope

This plan covers:

- making the public runtime invocation boundary canonical and unsurprising for
  embedders
- splitting `nimbus-runtime` by concept ownership instead of leaving dense
  multi-concept root files
- removing duplicated runtime invocation lifecycle logic across executor and
  worker paths
- splitting remaining dense engine surfaces by grouped concepts
- tightening visibility, helper placement, and state ownership toward more
  idiomatic Rust once structural seams stabilize
- updating architecture docs and verification notes for the landed ownership
  map

This plan does not cover:

- new product features
- intentional wire or route changes unless explicitly recorded
- storage format changes
- the Locker fork or cooperative runtime implementation itself
- admission-control redesign
- speculative performance rewrites without a clear modularity or correctness
  benefit

---

## Cleanup Invariants

These rules are mandatory for every item in this plan.

1. Preserve externally observable behavior by default.
   Public routes, runtime semantics, auth behavior, durable-journal semantics,
   and subscription semantics must stay unchanged unless a specific item says
   otherwise and the change is recorded.

2. Keep core architecture invariants intact.
   `nimbus-core` stays zero I/O.
   `nimbus-runtime` stays zero workspace dependencies.
   All mutations still flow through `Service::apply_mutation` or its queued
   async journal path.
   Storage atomicity stays unchanged.

3. Prefer concept-owned modules over helper piles.
   A successful split is one where a reader can quickly answer who owns the
   behavior, not one where code simply moved into more files.

4. Remove duplicated lifecycle logic before adding new abstraction layers.
   Shared runtime invocation semantics should have one canonical home.

5. Keep shutdown, cancellation, fairness, and delivery ordering semantics
   explicit and testable.

6. Add focused regression coverage before moving high-risk runtime or
   subscription ownership seams.

7. Treat the current git worktree as baseline reality.
   Resume from the code and this plan's ledger, not from chat memory.

---

## Current Assessed State

This plan assumes the codebase described in `ARCHITECTURE.md` today:

- `service/queries.rs` is the public read-path composition root over
  `service/queries/authorization.rs`, `planner.rs`, `prepared.rs`,
  `materialized.rs`, and `snapshot.rs`.
- `service/subscriptions.rs` owns subscribe and unsubscribe lifecycle, while
  `service/subscriptions/bootstrap.rs` owns initial evaluation and activation
  handoff.
- `tenant.rs` is the `TenantRuntime` facade and composition root over
  extracted tenant subsystems. It is no longer the primary cleanup hotspot.
- `runtime.rs` still owns public runtime types, bundle handling, bootstrap,
  op registration, and unmanaged invocation mechanics.
- `executor.rs` still owns queueing, per-tenant admission, worker threads,
  shared permit state, and watchdog ownership.
- `worker_loop.rs` still supplies the worker-loop seam that the runtime
  executor drives today and that the Locker workstream must preserve.

The current hotspot map from the live worktree is:

- `crates/nimbus-runtime/src/runtime.rs` is 3620 lines
- `crates/nimbus-runtime/src/executor.rs` is 2292 lines
- `crates/nimbus-engine/src/service/queries.rs` is 1173 lines
- `crates/nimbus-engine/src/subscriptions.rs` is 709 lines
- `crates/nimbus-engine/src/tenant.rs` is 523 lines and no longer a primary
  cleanup target

The runtime layer is now the dominant modularity risk. Engine cleanup still
matters, but the runtime boundary and lifecycle ownership are the highest-value
next moves.

---

## Current Review Findings

These findings describe the current reasons this plan exists.

1. Public runtime convenience APIs still construct a fresh `RuntimeExecutor`
   for each invocation instead of enforcing one canonical runtime execution
   ownership model. That makes the public `NimbusRuntime` surface easy to use
   in a non-canonical way and obscures the intended pooled execution model.
   Sources: `crates/nimbus-runtime/src/runtime.rs:1529`,
   `crates/nimbus-runtime/src/runtime.rs:1554`,
   `crates/nimbus-runtime/src/executor.rs:856`,
   `crates/nimbus-runtime/src/lib.rs:15`

2. Runtime invocation lifecycle logic is duplicated between the direct executor
   path and the worker-loop path. Metrics, cancellation, permit handling, and
   execution accounting can drift because they are expressed in parallel
   shapes.
   Sources: `crates/nimbus-runtime/src/executor.rs:925`,
   `crates/nimbus-runtime/src/worker_loop.rs:103`

3. `runtime.rs` still mixes public invocation types, bundle integrity
   handling, host-op payload schemas, op registration, bootstrap JavaScript,
   isolate setup, and runtime invocation logic in one file. The seams exist,
   but the file is still not grouped by concept ownership.
   Sources: `crates/nimbus-runtime/src/runtime.rs:31`,
   `crates/nimbus-runtime/src/runtime.rs:486`,
   `crates/nimbus-runtime/src/runtime.rs:1124`,
   `crates/nimbus-runtime/src/runtime.rs:1470`

4. `service/queries.rs` still combines multiple concept surfaces in one public
   root: document access, query and pagination access, durable-journal reads,
   shadow-materializer bootstrap, consistency verification, and test hooks.
   It is cleaner than before, but still denser than the architecture it fronts.
   Sources: `crates/nimbus-engine/src/service/queries.rs:65`,
   `crates/nimbus-engine/src/service/queries.rs:724`,
   `crates/nimbus-engine/src/service/queries.rs:813`,
   `crates/nimbus-engine/src/service/queries.rs:930`

5. `subscriptions.rs` still bundles registry state, dependency derivation,
   batch wakeup coalescing, policy invalidation, and delivery reevaluation in
   one file. The behavior is explicit, but the ownership model is still too
   dense for safe future iteration.
   Sources: `crates/nimbus-engine/src/subscriptions.rs:79`,
   `crates/nimbus-engine/src/subscriptions.rs:151`,
   `crates/nimbus-engine/src/subscriptions.rs:281`,
   `crates/nimbus-engine/src/subscriptions.rs:534`

---

## Success Criteria

This plan is successful only when all of the following are true:

1. The current feature set and stable behavior still work after the cleanup.
2. The public runtime invocation model is canonical and no longer invites
   accidental per-call executor construction.
3. `nimbus-runtime/src/runtime.rs` is split around concept ownership rather
   than continuing as a multi-concept root file.
4. Runtime executor and worker lifecycle semantics have one canonical home and
   no longer drift across parallel implementations.
5. Engine read surfaces are grouped by concept instead of mixing production
   APIs, verification helpers, journal reads, and test hooks in one root.
6. Subscription registry, dependency, wakeup, and delivery ownership are
   explicit enough to evolve safely.
7. The landed module map is reflected in docs and in a resumable execution log
   so a future agent can continue from this plan and the worktree alone.
8. Final verification proves the cleanup did not introduce regressions.

---

## Feature Preservation Matrix

Every implementation item must preserve these surfaces.

| Surface | Must stay true | Minimum item-level verification |
| --- | --- | --- |
| Native CRUD, query, paginated query, schema, scheduler, and journal routes | route semantics and durable behavior stay unchanged | targeted engine tests; `cargo test -p nimbus-server` for touched HTTP paths |
| Native WebSocket subscriptions | initial bootstrap, live delivery, cleanup on disconnect, and unsubscribe behavior stay unchanged | targeted engine and server reactive tests |
| Convex runtime query, mutation, action, scheduler, and nested-call paths | host-call semantics, error mapping, and executor ownership semantics stay unchanged unless explicitly recorded | targeted runtime and server Convex tests |
| Runtime admission, cancellation, timeout, and fairness semantics | queued, active, cancelled, timed-out, and shutdown behavior stay unchanged | targeted runtime tests plus `cargo test -p nimbus-runtime` |
| Authorization and policy-aware invalidation | read filters, policy invalidation, and principal-aware behavior stay unchanged | targeted engine and server authorization tests |
| Materialized read surface, diagnostics, and metrics snapshots | serving behavior and snapshot shapes stay unchanged unless explicitly documented | targeted engine tests; diagnostics or metrics tests when touched |

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

1. Reread `Cleanup Invariants`, `Current Assessed State`,
   `Feature Preservation Matrix`, `Roadmap Status Ledger`,
   `Implementation Checkpoints`, `Dependency Graph`,
   `Recommended Delivery Order`, and `Execution Log`.
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
  architecture-level ownership change promised by this plan

### Suggested autonomous prompt

```text
Use docs/plans/modularity-and-idiomatic-rust-cleanup-plan.md as the control
plane. Reread Cleanup Invariants, Current Assessed State,
Feature Preservation Matrix, Control Plane Rules, Verification Contract,
Roadmap Status Ledger, Implementation Checkpoints, Dependency Graph,
Recommended Delivery Order, and Execution Log, then inspect the current git
worktree. If any item is in_progress, resume it first. Reconcile dirty
worktree changes to the owning item before starting new scope. Implement
exactly one item, run the required verification, update the ledger,
checkpoint, and log, and continue. If blocked, record the blocker in the plan
before stopping. Do not rely on chat history as progress state.
```

---

## Verification Contract

### Minimum verification per implementation item

- targeted tests for the touched subsystem
- `cargo check --workspace`
- `cargo fmt --all --check`

### Additional verification by scope

- for runtime invocation, runtime module, or executor ownership refactors:
  `cargo test -p nimbus-runtime`
- for engine read-surface or subscription ownership refactors:
  `cargo test -p nimbus-engine`
- for server ownership, HTTP, WebSocket, or Convex bridge fallout:
  `cargo test -p nimbus-server`
- before marking any item `done`:
  `cargo clippy --workspace --all-targets -- -D warnings`

### Final verification before closing the plan

- `make check`
- `make test`
- `make clippy`

If `make ci` is practical at the end of the workstream, record that too.

If environmental limits block a command, record the limitation in
`Execution Log` and continue with the best focused verification available.

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| MC0 | done | Baseline review and hotspot map for the current modularity cleanup pass | none |
| MC1 | done | Make the public runtime invocation boundary canonical and eliminate per-call executor construction footguns | MC0 |
| MC2 | done | Split `nimbus-runtime/src/runtime.rs` into concept-owned modules for invocation types, host-op ABI, bootstrap, and runtime construction | MC0, MC1 |
| MC3 | done | Decompose runtime executor and worker lifecycle code so admission, queueing, permit state, and execution accounting have clear ownership with no duplicated semantics | MC0, MC1 |
| MC4 | done | Split engine read-service surfaces by grouped concepts instead of keeping documents, journal, verification, and test hooks in one root module | MC0 |
| MC5 | done | Decompose subscription ownership around registry, dependency derivation, batch wakeups, delivery, and policy invalidation | MC0, MC4 |
| MC6 | done | Perform an idiomatic Rust cleanup sweep after the main ownership boundaries stabilize | MC1, MC2, MC3, MC4, MC5 |
| MC7 | done | Architecture/docs update plus full verification sweep | MC1, MC2, MC3, MC4, MC5, MC6 |

---

## Dependency Graph

- `MC0` is the planning and hotspot baseline for the workstream.
- `MC1` should land first because it defines the canonical runtime ownership
  model that `MC2` and `MC3` must preserve.
- `MC2` depends on `MC1` because the runtime module tree should reflect the
  public invocation boundary rather than freezing the current convenience-path
  ambiguity into more files.
- `MC3` depends on `MC1` for the same reason and benefits from whatever
  `MC2` clarifies about runtime concept ownership.
- `MC4` can proceed once the current hotspot baseline is established; it does
  not need to wait on runtime cleanup.
- `MC5` depends on `MC4` because subscription ownership should not be split
  again before the read-service public root settles.
- `MC6` waits until the structural items land so visibility, helper placement,
  and duplication cleanup happen against the final ownership map.
- `MC7` is the closure pass that updates docs, removes leftover glue, and runs
  the full verification sweep.

---

## Recommended Delivery Order

1. `MC1`
2. `MC2`
3. `MC3`
4. `MC4`
5. `MC5`
6. `MC6`
7. `MC7`

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| MC0 | done; reviewed the current architecture and reformatted this workstream into a standalone control plane with explicit invariants, success criteria, preservation matrix, dependency graph, write-back rules, and resume guidance for future agents | start `MC1` by deciding the canonical public runtime ownership model and eliminating the per-call `RuntimeExecutor::new(...)` convenience path |
| MC1 | done; `NimbusRuntime` now lazily owns a shared executor for its public convenience entrypoints, `invoke_bundle*` routes through the worker pool instead of constructing a fresh executor per call, and runtime coverage now proves convenience invocations reuse pooled worker state without changing observable results | start `MC2` by turning `runtime.rs` into a concept-owned module tree rooted in invocation types, host-op ABI, bootstrap, and runtime construction |
| MC2 | done; `runtime.rs` is now a runtime composition root over `runtime/invocation.rs`, `runtime/bundle.rs`, and `runtime/bootstrap.rs`, so public invocation/auth payloads, bundle identity and integrity handling, and bootstrap snapshot plus host-op ABI registration no longer live inline beside runtime construction and unmanaged invocation logic | start `MC3` by mapping the remaining ownership seams in `executor.rs` and `worker_loop.rs`, then extract admission, queue, permit, and invocation lifecycle code into dedicated modules without changing fairness or shutdown semantics |
| MC3 | done; `executor.rs` is now the composition root over `executor/admission.rs`, `executor/queue.rs`, and `executor/lifecycle.rs`, so admission and tenant fairness, queue and shutdown plumbing, and the shared invocation lifecycle no longer live inline in one file, and `worker_loop.rs` now runs through the same lifecycle helper as the direct executor path to keep cancellation, timeout, metric, and permit-finish semantics canonical | start `MC4` by mapping the remaining grouped concepts in the engine read-service root, then split document/query APIs, durable-journal reads, verification helpers, and test hooks into dedicated modules without changing route or result semantics |
| MC4 | done; `queries.rs` is now a narrow composition root over `queries/documents.rs`, `query_api.rs`, `journal.rs`, `verification.rs`, and `test_hooks.rs`, so the public read surface is grouped by concept instead of mixing document/list reads, query and pagination entrypoints, durable-journal access, consistency helpers, and test-only hooks in one file; the existing private planner, authorization, materialized-surface, and snapshot modules stayed in place under the same read path | start `MC5` by mapping `subscriptions.rs` into concept-owned seams for registry state, dependency derivation, batch wakeups, delivery, and policy invalidation without changing bootstrap or disconnect cleanup behavior |
| MC5 | done; `subscriptions.rs` is now a composition root over `subscriptions/registry.rs`, `dependencies.rs`, `queue.rs`, `delivery.rs`, and `invalidation.rs`, so registration state, dependency scans, queued wakeup coalescing, reevaluation and monotonic delivery, and policy or shutdown teardown no longer share one file; `service/subscriptions.rs` kept the bootstrap and transport-facing entrypoint surface stable while engine and server subscription behavior stayed unchanged | start `MC6` by tightening visibility, helper placement, and avoidable glue across the stabilized runtime, read-service, and subscription module trees |
| MC6 | done; cleaned the stabilized module trees without blurring ownership again: `service/queries/journal.rs` now has a single async journal-read helper instead of repeating read-storage plumbing, `service/subscriptions.rs` now centralizes pending-registration and bootstrap publish glue, `subscriptions/registry.rs` uses a single local mutation helper for activation and delivery bookkeeping, and `executor/queue.rs` now shares closed-executor sender lookup and ready-job failure handling in one place | start `MC7` by confirming the docs and plan index still reflect the active workstream, then run the repo-wide `make check`, `make test`, and `make clippy` sweep |
| MC7 | done; `ARCHITECTURE.md`, `docs/plans/README.md`, and `AGENTS.md` still match the landed ownership map, the control plan now reflects MC0 through MC7 as complete, and the repo-wide verification sweep is recorded; the only remaining work after this document is external review or an explicit archival follow-up if requested | workstream complete; use this plan as historical completion state until a new cleanup plan is promoted or this plan is explicitly archived |

---

## Work Items

### MC0. Baseline review and hotspot map

Completed in this planning pass.

Acceptance criteria:

- current hotspots are identified from the actual live worktree
- the next cleanup sequence is driven by concept ownership, not raw LOC
- the plan is self-sufficient enough to resume after compaction or handoff

### MC1. Canonical runtime invocation boundary

#### Implementation plan

1. Decide the intended public runtime ownership model for embedders:
   - explicit `RuntimeExecutor` ownership for pooled invocation, or
   - a stable executor owned behind `NimbusRuntime`
2. Eliminate the current public path that constructs a fresh executor for each
   `invoke_bundle*` call.
3. Keep the runtime and worker-loop architecture aligned with the intended
   public ownership model.
4. Add focused runtime tests that prove the canonical path does not spin up a
   new executor shape per invocation.

#### Focused verification

- targeted runtime tests covering public invocation entrypoints
- `cargo test -p nimbus-runtime`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- the public runtime surface has one clear, canonical invocation path
- pooled execution semantics are not optional by accident
- existing runtime behavior and error mapping stay unchanged

### MC2. Split `runtime.rs` by concept ownership

#### Implementation plan

1. Keep `runtime.rs` as a thin composition root or replace it with a module
   tree rooted at `runtime/mod.rs`.
2. Extract grouped concepts into dedicated modules, likely including:
   - invocation request and identity types
   - runtime bundle and integrity handling
   - host-op payload and op registration
   - bootstrap JavaScript and startup snapshot handling
   - runtime construction and unmanaged invocation
3. Keep the typed `HostCallOperation` contract as the single operation
   registry.
4. Move tests toward the modules that own the behavior when that improves
   clarity.

#### Focused verification

- targeted runtime tests for bundle loading, op registration, and invocation
  plumbing
- `cargo test -p nimbus-runtime`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- `runtime.rs` no longer owns unrelated runtime concepts in one file
- runtime host ABI and bootstrap code are no longer entangled with unrelated
  public types
- public exports remain intentional and understandable

### MC3. Decompose executor and worker lifecycle ownership

#### Implementation plan

1. Split `executor.rs` into concept-owned modules such as:
   - admission and tenant fairness
   - queue and dispatch plumbing
   - shared permit and parked invocation state
   - public executor entrypoints
2. Remove duplicated invocation lifecycle logic between the direct executor path
   and `worker_loop.rs`.
3. Keep metrics, cancellation, fairness, timeout, and shutdown semantics in one
   canonical ownership path.
4. Preserve the `WorkerLoopFactory` seam for the Locker fork plan.

#### Focused verification

- targeted runtime tests for fairness, cancellation, timeouts, and shutdown
- `cargo test -p nimbus-runtime`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- runtime lifecycle semantics have one canonical home
- worker-loop and executor code no longer drift independently
- fairness, cancellation, and shutdown behavior stay unchanged

### MC4. Split engine read-service surfaces by grouped concepts

#### Implementation plan

1. Keep the existing private query capability tree from the current
   architecture.
2. Split the public `Service` read root by grouped concept surfaces, likely:
   - document and list access
   - query and pagination access
   - durable-journal access
   - shadow and consistency verification
   - test hooks
3. Keep sync, async, and cancellable behavior aligned per concept surface.
4. Avoid moving large logic blocks back into one public root just to preserve
   legacy file names.

#### Focused verification

- targeted engine tests for CRUD, query, pagination, and journal reads
- `cargo test -p nimbus-engine`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- read CRUD and query APIs are not mixed with verification and journal
  utilities in one file
- test hooks are quarantined from normal production API reading paths
- public behavior remains unchanged

### MC5. Decompose subscription ownership by concept

#### Implementation plan

1. Split `subscriptions.rs` into grouped modules, likely:
   - registry and registration cleanup
   - dependency derivation and affected-id scans
   - queued work and coalescing
   - delivery dispatch and reevaluation
   - policy invalidation and shutdown
2. Keep the current monotonic delivery and narrowed dependency semantics.
3. Preserve the explicit boundary between engine subscription ownership and
   server transport and session ownership.
4. Add focused coverage for any moved wakeup or delivery logic.

#### Focused verification

- targeted engine and server subscription tests for bootstrap, delivery, and
  disconnect cleanup
- `cargo test -p nimbus-engine`
- `cargo test -p nimbus-server`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- registry, dependency, and delivery concepts have clearer ownership
- policy invalidation and shutdown paths remain explicit
- no subscription behavior regresses

### MC6. Idiomatic Rust cleanup sweep

#### Implementation plan

1. Narrow visibility where module boundaries now allow it.
2. Move ad hoc helper structs and enums beside the concept they support.
3. Remove avoidable duplication, clone-heavy glue, and ad hoc state plumbing
   only where it improves readability.
4. Prefer small explicit helpers over macro-heavy indirection unless a macro
   clearly improves the operation registry or repetitive boilerplate.

#### Focused verification

- targeted tests for each touched subsystem
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- the cleaned modules read more idiomatically and locally
- no abstraction-for-its-own-sake layer is introduced
- clippy stays green under `-D warnings`

### MC7. Docs and full verification sweep

#### Implementation plan

1. Update `ARCHITECTURE.md` to reflect the landed ownership map.
2. Update `docs/plans/README.md` and any affected indexes.
3. Record the actual completion state in this plan.
4. Run the repo-wide verification contract.

#### Focused verification

- `make check`
- `make test`
- `make clippy`

#### Acceptance criteria

- docs match the landed code structure
- this plan is resumable without chat history
- verification is recorded and green

---

## Execution Log

Append new rows at the top. Keep entries short and factual.

| Date | Item | Outcome | Summary | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-03 | MC7 | done | Closed the modularity and idiomatic Rust cleanup workstream with the final repo-wide verification sweep and plan reconciliation. Confirmed `ARCHITECTURE.md` reflects the landed runtime, read-service, and subscription ownership maps; confirmed `docs/plans/README.md` and `AGENTS.md` still point to the correct active plan entrypoints for this completed workstream state; and updated the control plane so MC0 through MC7 now match the codebase and verification history. | `make check`; `make test`; `make clippy` | workstream complete; archive or supersede this plan only when a new user task explicitly asks for that follow-up |
| 2026-04-03 | MC6 | done | Performed the idiomatic Rust sweep against the stabilized runtime, read-service, and subscription module trees without introducing new abstraction layers. Centralized async journal read-storage plumbing in `service/queries/journal.rs`, pulled duplicated pending-registration and bootstrap publish glue into local helpers in `service/subscriptions.rs`, added a small registry mutation helper in `subscriptions/registry.rs`, and deduplicated closed-executor sender lookup plus ready-job failure handling in `nimbus-runtime/src/executor/queue.rs`. | `cargo check -p nimbus-runtime`; `cargo check -p nimbus-engine`; `cargo test -p nimbus-runtime`; `cargo test -p nimbus-engine`; `cargo check --workspace`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | start `MC7` by running the repo-wide `make check`, `make test`, and `make clippy` sweep and reconciling the remaining docs state |
| 2026-04-03 | MC5 | done | Split tenant-local subscription ownership by concept instead of keeping registry state, dependency matching, queued wakeup coalescing, delivery dispatch, and teardown or policy invalidation in one `subscriptions.rs` file. Added `subscriptions/registry.rs`, `dependencies.rs`, `queue.rs`, `delivery.rs`, and `invalidation.rs`; reduced `subscriptions.rs` to a composition root over those concepts; and updated `ARCHITECTURE.md` to describe the new ownership map while preserving the existing `service/subscriptions.rs` bootstrap boundary and all subscription behavior. | `cargo test -p nimbus-engine async_subscription_bootstrap_catches_up_writes_committed_before_activation`; `cargo test -p nimbus-engine subscription_updates_publish_only_after_journal_apply`; `cargo test -p nimbus-engine service_unsubscribe_stops_notifications`; `cargo test -p nimbus-engine policy_revision_changes_terminate_active_authorized_subscriptions`; `cargo test -p nimbus-server socket::subscriptions::websocket_disconnect_before_bootstrap_activation_cancels_pending_subscription_and_reconnects_cleanly`; `cargo test -p nimbus-server socket::subscriptions::websocket_unsubscribe_stops_receiving_updates`; `cargo test -p nimbus-server socket::subscriptions::websocket_disconnect_drops_subscription_without_explicit_unsubscribe`; `cargo test -p nimbus-engine`; `cargo test -p nimbus-server`; `cargo check --workspace`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | start `MC6` by running the idiomatic Rust sweep now that the major ownership boundaries have stabilized |
| 2026-04-03 | MC4 | done | Split the engine read-service public root by grouped concepts instead of keeping document/list reads, query and pagination entrypoints, durable-journal access, consistency verification helpers, and test hooks in one `queries.rs` file. Added `queries/documents.rs`, `query_api.rs`, `journal.rs`, `verification.rs`, and `test_hooks.rs`; reduced `queries.rs` to a composition root over those surfaces plus the existing private planner/materialized capability tree; and updated `ARCHITECTURE.md` to record the new ownership map without changing read behavior or test-hook contracts. | `cargo test -p nimbus-engine service_missing_document_operations_return_not_found`; `cargo test -p nimbus-engine query_uses_index_for_equality_filter`; `cargo test -p nimbus-engine paginate_with_cursor_returns_next_page`; `cargo test -p nimbus-engine durable_journal_reads_return_strictly_ordered_authoritative_records`; `cargo test -p nimbus-engine`; `cargo check --workspace`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | start `MC5` by splitting subscription ownership around registry state, dependency derivation, batch wakeups, delivery, and policy invalidation |
| 2026-04-03 | MC3 | done | Split runtime executor ownership around grouped concepts instead of keeping admission, queueing, and invocation lifecycle semantics intertwined in `executor.rs`. Added `executor/admission.rs` for tenant fairness and shared permit ownership, `executor/queue.rs` for worker queue and shutdown plumbing, and `executor/lifecycle.rs` for the canonical invocation lifecycle path used by both direct executor entrypoints and `worker_loop.rs`. Updated `ARCHITECTURE.md` to describe `executor.rs` as the composition root over those concept-owned modules while preserving `WorkerLoopFactory` and existing runtime semantics. | `cargo check -p nimbus-runtime`; `cargo test -p nimbus-runtime`; `cargo check --workspace`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | start `MC4` by splitting the engine read-service root around document/query APIs, journal reads, verification helpers, and test hooks |
| 2026-04-03 | MC2 | done | Split `nimbus-runtime/src/runtime.rs` around grouped concepts instead of keeping invocation/auth payloads, bundle identity and integrity handling, and bootstrap plus host-op ABI registration inline in one root file. Added `runtime/invocation.rs`, `runtime/bundle.rs`, and `runtime/bootstrap.rs`; kept `runtime.rs` as the composition root for public runtime construction, the runtime-owned convenience executor boundary, and unmanaged invocation; updated `ARCHITECTURE.md` to describe the new module ownership map; and kept the public exports stable. `runtime.rs` is down to 2192 lines total including tests, with the moved concepts now living in dedicated sibling modules. | `cargo check -p nimbus-runtime`; `cargo test -p nimbus-runtime`; `cargo check --workspace`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | start `MC3` by decomposing runtime executor and worker lifecycle ownership now that the runtime surface and bootstrap ABI are extracted |
| 2026-04-03 | MC1 | done | Made the public runtime convenience path canonical without spawning duplicate executor pools in explicit server-owned executor flows. `NimbusRuntime` now lazily owns a shared `RuntimeExecutor`, `invoke_bundle*` routes through the runtime-owned worker pool, and `ARCHITECTURE.md` now records that standalone convenience calls use pooled executor ownership by default while explicit server integrations still inject their own shared executor. Added `convenience_runtime_invocations_reuse_runtime_owned_executor` to prove repeated convenience calls reuse the same worker-local isolate pool instead of constructing fresh executors or bypassing pooled execution. | `cargo test -p nimbus-runtime convenience_runtime_invocations_reuse_runtime_owned_executor -- --nocapture`; `cargo test -p nimbus-runtime pooled_runtime_invocations_keep_module_state_fresh -- --nocapture`; `cargo test -p nimbus-runtime`; `cargo check --workspace`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | start `MC2` by splitting `runtime.rs` into concept-owned modules now that the public invocation boundary is stable |
| 2026-04-03 | MC0 | done | Reformatted this workstream into a standalone control plane anchored in the current architecture and current hotspots. Added explicit success criteria, a feature-preservation matrix, a dependency graph, required write-back rules, focused verification guidance per item, and a suggested autonomous resume prompt. Updated `AGENTS.md` in the same pass so new agents resume from this plan and the current worktree instead of chat state. | docs-only review and plan rewrite; no code verification rerun | start `MC1` by making the public runtime invocation boundary canonical and eliminating the per-call executor construction path |
| 2026-04-03 | MC0 | done | Reviewed the current architecture and mapped the next cleanup hotspots from the live worktree. Confirmed that the primary remaining modularity pressure is now in `nimbus-runtime`: the public runtime convenience APIs still construct a fresh executor per invocation, `runtime.rs` still mixes multiple unrelated concepts, and `executor.rs` still duplicates invocation lifecycle semantics with `worker_loop.rs`. Confirmed the next engine hotspots are `service/queries.rs` and `subscriptions.rs`, while `tenant.rs` is no longer a primary cleanup target. Authored the initial control plan to drive concept-oriented modularity and idiomatic Rust cleanup from the current architecture and current hotspots. | source review of `README.md`, `ARCHITECTURE.md`, `docs/README.md`, `docs/plans/README.md`, `crates/nimbus-runtime/src/runtime.rs`, `crates/nimbus-runtime/src/executor.rs`, `crates/nimbus-runtime/src/worker_loop.rs`, `crates/nimbus-engine/src/service/queries.rs`, and `crates/nimbus-engine/src/subscriptions.rs` | keep `MC0` done and start `MC1` by making the public runtime invocation boundary canonical |
