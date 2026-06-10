# Operational State And Scenario Surface Cleanup Control Plan

Archived on 2026-04-06 after `OS0` through `OS7` completed. This file is a
historical record, not an active control plane.

This is the canonical execution control plane for the current operational
state, evaluator, websocket-session, metrics, and scenario-surface cleanup
workstream.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `crates/nimbus-engine/src/tenant/mutation.rs`
- `crates/nimbus-storage/src/store/write.rs`
- `crates/nimbus-engine/src/evaluator.rs`
- `crates/nimbus-runtime/src/metrics.rs`
- `crates/nimbus-server/src/ws/socket.rs`
- `crates/nimbus-server/src/adapters/convex/subscriptions/socket/named_subscriptions.rs`
- `crates/nimbus-engine/src/tests.rs`
- `crates/nimbus-storage/src/tests.rs`

Baseline verification status for this plan:

- the immediately preceding deep module-ownership cleanup workstream closed
  green on 2026-04-03 with:
  `cargo fmt --all --check`,
  `cargo check --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `make check`,
  `make test`,
  and `make clippy`
- `make ci` was also attempted during that closure pass, but the local
  environment did not have `cargo deny` installed, so the `deny` step could
  not run locally
- this control plane was authored as a docs-only review-and-planning pass on
  2026-04-04 before any `OS*` implementation work landed

---

## Purpose

The previous cleanup passes removed the highest-level god modules and replaced
them with thinner composition roots. The next layer of worthwhile cleanup is
deeper and more stateful: files that still combine queue ownership, wait
accounting, metrics aggregation, cursor semantics, websocket-session lifecycle,
or giant concept-mixed test roots.

This plan exists to make those inner ownership seams explicit without
repeating the earlier mistake of splitting files only to lower line counts. The
goal is concept-owned modules, canonical naming, clearer state ownership,
readable Rust, and easier debugging and feature work.

---

## Relationship To Other Plans

- This plan succeeds
  `docs/plans/archive/deep-module-ownership-and-canonical-cleanup-plan.md`
  as the next active cleanup control plane.
- This plan is separate from
  `docs/plans/layered-admission-control-plan.md`,
  `docs/plans/encryption-at-rest-plan.md`,
  `docs/plans/archive/convex-demos-compatibility-plan.md`,
  and `docs/plans/v8-locker-fork-plan.md`.

---

## Scope

In scope:

- engine tenant mutation admission and journal state ownership
- storage write-path and transaction-lifecycle ownership
- evaluator, cursor, ordering, and pagination ownership
- runtime metrics ownership and snapshot assembly
- websocket-session and Convex named-subscription ownership
- remaining giant scenario and regression test surfaces
- final idiomatic Rust cleanup that falls out of the new ownership map

Out of scope unless the plan is explicitly amended first:

- behavior-changing API, route, wire, or policy changes
- new admission-control policy or `EO*`-style boundary promotion work
- encryption-at-rest implementation work
- the V8 Locker fork workstream
- compatibility layers for pre-launch behavior

---

## Cleanup Invariants

- `nimbus-core` remains zero-I/O.
- `nimbus-runtime` remains zero-workspace-dependency.
- All mutations still flow through `Service::apply_mutation` or its queued
  async journal path.
- Storage atomicity remains intact: document write, index maintenance, and
  durable journal append stay in one redb transaction.
- Runtime host operations still flow through the same service mutation/query
  paths, not bypasses.
- Query, pagination, ordering, and cursor semantics remain unchanged.
- WebSocket subscription bootstrap, unsubscribe, disconnect cleanup, and live
  delivery semantics remain unchanged.
- Runtime metrics and diagnostics route shapes remain unchanged unless a change
  is explicitly recorded in this plan.
- No compatibility code for pre-launch legacy behavior is allowed.

---

## Current Assessed State

- The biggest top-level composition roots were already split in the previous
  cleanup workstreams, and the codebase is materially easier to navigate than
  it was before.
- `crates/nimbus-runtime/src/runtime.rs` and
  `crates/nimbus-runtime/src/executor.rs` are still large, but they are now
  mostly composition plus inline tests; they are not the highest-value next
  split targets.
- The remaining high-value cleanup is concentrated in inner stateful files that
  still own multiple operational concepts at once.
- The remaining highest-value test cleanup is concentrated in
  `crates/nimbus-engine/src/tests.rs` and
  `crates/nimbus-storage/src/tests.rs`, which still carry many concept-mixed
  scenario clusters.
- The current worktree should be committed before any `OS1+` implementation
  work starts so this plan becomes the new durable control plane from a clean
  handoff point.

---

## Current Review Findings

1. `crates/nimbus-engine/src/tenant/mutation.rs` still combines queued request
   models, admission gating, CoDel shedding, journal queue state, wait
   accounting, and test pause ownership in one file.
2. `crates/nimbus-storage/src/store/write.rs` still combines
   `TenantWriteTransaction`, direct CRUD helpers, batch/durable commit helpers,
   scheduled-write integration, and `TenantStore` open/create lifecycle.
3. `crates/nimbus-engine/src/evaluator.rs` still combines filtering, document
   ordering, cursor encoding/validation, paginated windowing, and store-backed
   versus preloaded-document evaluation paths.
4. `crates/nimbus-runtime/src/metrics.rs` still combines global runtime
   counters, host-operation metrics, per-tenant metrics, duration
   distributions, request-correlation retention, and snapshot assembly.
5. `crates/nimbus-server/src/ws/socket.rs` and
   `crates/nimbus-server/src/adapters/convex/subscriptions/socket/named_subscriptions.rs`
   still mix websocket session transport, bootstrap cancellation tracking,
   runtime/native subscription activation, and initial publish/forwarding
   concerns.
6. `crates/nimbus-engine/src/tests.rs` and
   `crates/nimbus-storage/src/tests.rs` still hold large concept-mixed test
   inventories that slow local comprehension and make feature-oriented updates
   harder than they should be.

---

## Success Criteria

- remaining stateful hotspots have concept-owned module boundaries
- major operational state owners are easier to name, test, and reason about
- giant test roots are materially smaller because concept-owned surfaces now
  hold the highest-value scenario clusters
- naming, visibility, and helper placement are more idiomatic and consistent
- no externally observable behavior changes are introduced unintentionally
- the plan can be archived cleanly once the workstream is complete

---

## Feature Preservation Matrix

- Mutation admission, queued journal durability, and applied-visibility
  semantics must remain unchanged.
- Storage write-path atomicity, scheduled-job durability, and execution-unit
  batch semantics must remain unchanged.
- Query evaluation, sorting, pagination, and cursor validation semantics must
  remain unchanged.
- Runtime metrics route shapes and runtime cancellation/fairness semantics must
  remain unchanged unless explicitly documented in this plan.
- Native websocket subscription bootstrap, unsubscribe, disconnect cleanup,
  and live push semantics must remain unchanged.
- Convex named-subscription bootstrap, initial publish, transform handling, and
  runtime base-query forwarding semantics must remain unchanged.
- Existing broad storage, engine, and server integration coverage must remain
  intact even when tests move to concept-owned surfaces.

---

## Control Plane Rules

### Status model

- `todo`: not started
- `in_progress`: actively being implemented; exactly one item may hold this
  status at a time
- `done`: implemented, verified, and reflected in the checkpoints and log
- `blocked`: cannot proceed until an explicitly recorded blocker is resolved

### Recovery loop for every new session or post-compaction resume

1. Read `AGENTS.md`.
2. Read `docs/plans/README.md`.
3. Read this file end to end, especially `Cleanup Invariants`,
   `Current Assessed State`, `Current Review Findings`,
   `Feature Preservation Matrix`, `Control Plane Rules`,
   `Verification Contract`, `Roadmap Status Ledger`,
   `Implementation Checkpoints`, `Dependency Graph`,
   `Recommended Delivery Order`, and `Execution Log`.
4. Inspect the current git worktree.
5. If any `OS*` item is already `in_progress`, resume it first.
6. Otherwise pick the first eligible item in `Recommended Delivery Order`.
7. Mark the chosen item `in_progress` before implementation begins.

### Dirty-worktree reconciliation rules

- Reconcile dirty files to the owning roadmap item before starting new scope.
- Do not skip ahead while an earlier eligible item remains `todo`.
- If a change belongs to a different workstream or unrelated user dirt, leave
  it alone unless explicitly asked to handle it.

### Non-deviation rules

- Do not change behavior intentionally unless this plan is amended first.
- Do not add compatibility shims for pre-launch behavior.
- Prefer extraction into concept-owned modules over helper piles.
- Keep composition roots thin once ownership moves elsewhere.
- Update this plan before taking a materially better implementation path than
  what is recorded here.

### Required write-back after each work session

- update the item status in `Roadmap Status Ledger`
- update or add the item note in `Implementation Checkpoints` if the item
  remains partial
- append a row to `Execution Log` with date, item, outcome, verification, and
  next step
- update `ARCHITECTURE.md` in the same change set when architecture-level
  ownership changes land

### Suggested autonomous prompt

```text
Use docs/plans/operational-state-and-scenario-surface-cleanup-plan.md as the
control plane. Reread Cleanup Invariants, Current Assessed State,
Current Review Findings, Feature Preservation Matrix, Control Plane Rules,
Verification Contract, Roadmap Status Ledger, Implementation Checkpoints,
Dependency Graph, Recommended Delivery Order, and Execution Log, then inspect
the current git worktree. If any item is in_progress, resume it first.
Reconcile dirty worktree changes to the owning item before starting new scope.
Implement exactly one item, run the required verification, update the ledger,
checkpoint, and log, and continue. If blocked, record the blocker in the plan
before stopping. Do not rely on chat history as progress state.
```

---

## Verification Contract

### Minimum verification per implementation item

- `cargo fmt --all`
- the focused verification listed under the active item
- `cargo fmt --all --check`
- `cargo check --workspace`

### Additional verification by scope

- run `cargo test -p nimbus-engine` for engine-facing changes
- run `cargo test -p nimbus-storage` for storage-facing changes
- run `cargo test -p nimbus-runtime` for runtime-facing changes
- run `cargo test -p nimbus-server` for websocket, server, or Convex-facing
  changes
- run `cargo clippy --workspace --all-targets -- -D warnings` after meaningful
  implementation slices or before closing an item whose changes span crates

### Final verification before closing the plan

- `make check`
- `make test`
- `make clippy`
- attempt `make ci` if practical, and record any environment limitation if it
  cannot complete

If any required command is blocked by environment or sandbox restrictions,
record that explicitly in `Execution Log` and continue with the best focused
verification available.

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| OS0 | done | Baseline review and hotspot map for the next operational-state and scenario-surface cleanup pass | none |
| OS1 | done | Split `crates/nimbus-engine/src/tenant/mutation.rs` by mutation operational-state ownership | OS0 |
| OS2 | done | Decompose `crates/nimbus-storage/src/store/write.rs` around durable write-path ownership | OS0 |
| OS3 | done | Split `crates/nimbus-engine/src/evaluator.rs` by evaluation, sorting, cursor, and pagination ownership | OS0 |
| OS4 | done | Decompose `crates/nimbus-runtime/src/metrics.rs` around runtime metrics ownership | OS0 |
| OS5 | done | Normalize websocket-session and Convex named-subscription ownership across the remaining socket hotspots | OS0 |
| OS6 | done | Move the highest-value remaining test clusters to concept-owned surfaces and perform the follow-on idiomatic Rust sweep | OS1, OS2, OS3, OS4, OS5 |
| OS7 | done | Update docs and run the full verification closure sweep | OS1, OS2, OS3, OS4, OS5, OS6 |

---

## Dependency Graph

- `OS0` is the current architecture review and hotspot baseline.
- `OS1` and `OS2` lead because they target the clearest remaining state-heavy
  inner ownership seams in engine and storage.
- `OS3` follows early because evaluator ownership affects both engine read
  internals and later test movement.
- `OS4` targets the remaining dense runtime diagnostics surface once the new
  plan is underway.
- `OS5` normalizes transport and subscription-session ownership after the core
  engine/runtime state work is mapped.
- `OS6` waits until the production ownership boundaries land so test movement
  follows the stabilized structure.
- `OS7` is the closure pass that reconciles docs, archives the plan, and runs
  the full verification sweep.

---

## Recommended Delivery Order

1. `OS1`
2. `OS2`
3. `OS3`
4. `OS4`
5. `OS5`
6. `OS6`
7. `OS7`

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| OS0 | done; reviewed the live post-cleanup architecture, confirmed that the next high-value cleanup is concentrated in `tenant/mutation.rs`, `store/write.rs`, `evaluator.rs`, `runtime/metrics.rs`, the websocket session/subscription socket surfaces, and the remaining giant engine/storage test roots. Also confirmed that `runtime.rs` and `executor.rs` are no longer first-line god-file targets because they are now mostly composition plus inline tests. | commit the completed previous cleanup handoff plus this new active control plane, then start `OS1` by mapping `tenant/mutation.rs` into admission, journal-state, wait-accounting, and test-hook seams |
| OS1 | done; split `tenant/mutation.rs` into a thin composition root over `requests.rs`, `admission.rs`, `codel.rs`, `journal.rs`, `stats.rs`, and `pause.rs`, keeping `tenant.rs` as the stable facade while preserving mutation admission, queueing, wait-accounting, and test-hook semantics | start `OS2` by mapping `store/write.rs` into transaction-lifecycle, CRUD helper, durable-commit, scheduled-write, and store-construction seams; continue recording the temporary workspace-wide verification blocker from the parallel V8 fork workstream until it is resolved |
| OS2 | done; split `store/write.rs` into a thin composition root over `transaction.rs`, `scheduled.rs`, `direct.rs`, `batch.rs`, and `store_entry.rs`, separating transaction lifecycle, scheduled-write integration, direct CRUD helpers, execution-unit batch apply, and `TenantStore` write-entry ownership without changing durable write semantics | start `OS3` by mapping `evaluator.rs` into concept-owned modules for filter evaluation, ordering, cursor encoding or validation, paginated windowing, and store-backed versus preloaded evaluation surfaces; continue recording the temporary workspace-wide verification blocker from the parallel V8 fork workstream until it is resolved |
| OS3 | done; split `evaluator.rs` into a thin composition root over `query.rs`, `pagination.rs`, `filtering.rs`, `ordering.rs`, and `cursor.rs`, separating query or pagination surfaces from shared filter, order, and cursor semantics while preserving existing evaluator behavior | start `OS4` by mapping `runtime/metrics.rs` into global runtime counters, host-operation metrics, per-tenant metrics, duration-distribution ownership, correlation retention, and snapshot assembly; continue recording the temporary workspace-wide verification blocker from the parallel V8 fork workstream until it is resolved |
| OS4 | done; kept `RuntimeMetrics` as the stable facade over `global.rs`, `host_operations.rs`, `tenants.rs`, and `correlations.rs`, then closed the newly unblocked verification fallout by fixing busy-worker affinity routing, refreshing the convenience-runtime expectation to match the default startup-snapshot pool, restoring workspace `RuntimeLimits` initialization, and isolating the delayed-async snapshot repro behind a subprocess-backed test so the full runtime suite stays green under the locker fork | start `OS5` by mapping the remaining websocket-session and Convex named-subscription hotspots into transport-task, bootstrap-cancellation, registration-cleanup, and initial-publish ownership seams |
| OS5 | done; split `ws/socket.rs` into a thin composition root over `transport.rs`, `pending.rs`, and `session.rs`, then split Convex named-subscription handling into `named_subscriptions/direct.rs` and `named_subscriptions/runtime.rs` so websocket transport tasks, pending bootstrap cancellations, generic session lifecycle, and native versus runtime bootstrap or initial-publish ownership now have clear homes without changing unsubscribe, disconnect, or initial-publish semantics | start `OS6` by mapping the remaining engine and storage test clusters into concept-owned module-local surfaces, then tighten naming, visibility, and helper placement around the stabilized production boundaries |
| OS6 | done; moved the highest-value evaluator, scan, and durable-journal snapshot or recovery tests out of the giant crate roots and into `evaluator/tests.rs`, `store/scan/tests.rs`, `store/journal/tests.rs`, `store/journal_stream/tests.rs`, and `store/journal_snapshot/tests.rs`, then trimmed stale root-test helpers so the remaining root suites are more integration-focused and the module trees now own their own semantics | start `OS7` by reconciling the plan state with the landed module-local test surfaces, then run the repo-wide closure sweep and archive the completed plan |
| OS7 | done; reran the repo-wide guarded verification entrypoints, recorded the still-local `cargo deny` limitation from `make ci`, and reconciled the repo entrypoints so the completed cleanup plan can be archived without any active pointer still treating it as live progress state | archive this plan under `docs/plans/archive/`, add it to the archived list in `docs/plans/README.md`, and leave future cleanup work to a newly promoted plan instead of reviving this completed one |

---

## Work Items

### OS0. Baseline review and hotspot map

#### Outcome

- Completed during this planning pass.

### OS1. Split `tenant/mutation.rs` by mutation operational-state ownership

#### Implementation plan

1. Extract concept-owned modules for admission gating, CoDel shedding, queued
   request/response models, journal queue state, and test pause ownership.
2. Keep `tenant/mutation.rs` or `tenant/mutation/mod.rs` as the thin mutation
   state composition root.
3. Preserve metrics snapshot shapes, wait accounting, and queued journal
   semantics exactly.

#### Focused verification

- `cargo test -p nimbus-engine`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- mutation admission and journal state ownership are clearer and more local
- test hooks no longer sit in the middle of unrelated mutation state logic

### OS2. Decompose `store/write.rs` around durable write-path ownership

#### Implementation plan

1. Separate `TenantWriteTransaction` lifecycle and commit ownership from the
   public `TenantStore` construction/open helpers.
2. Split direct document CRUD helpers, batch/durable commit helpers, and
   scheduled-write integration into clearer concept-owned modules.
3. Keep transaction atomicity and the durable commit contract unchanged.

#### Focused verification

- `cargo test -p nimbus-storage`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- write-path ownership is easier to navigate
- store constructors no longer live in the middle of write-transaction logic

### OS3. Split `evaluator.rs` by evaluation, ordering, cursor, and pagination ownership

#### Implementation plan

1. Extract filter evaluation, document ordering, cursor encode/decode, and
   pagination/windowing into concept-owned modules.
2. Keep the public evaluator surface stable for the engine call sites.
3. Preserve cursor validation, ordering rules, and pagination semantics
   exactly.

#### Focused verification

- `cargo test -p nimbus-engine`
- `cargo test -p nimbus-server`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- cursor and pagination logic are easier to reason about in isolation
- evaluation paths no longer mix unrelated responsibilities in one file

### OS4. Decompose `runtime/metrics.rs` around runtime metrics ownership

#### Implementation plan

1. Extract global runtime counters, host-operation metrics, tenant metrics,
   duration distributions, request correlation retention, and snapshot assembly
   into concept-owned modules.
2. Keep the public metrics and snapshot shapes unchanged.
3. Preserve the current cheap relaxed-ordering diagnostic model.

#### Focused verification

- `cargo test -p nimbus-runtime`
- `cargo test -p nimbus-server`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- runtime diagnostics code is easier to extend without touching unrelated
  metric families
- metrics snapshots remain behaviorally identical

### OS5. Normalize websocket-session and named-subscription ownership

#### Implementation plan

1. Split generic websocket session transport, reader/writer task ownership,
   pending-bootstrap cancellation tracking, and subscription registration
   lifecycle into clearer modules.
2. Split Convex named-subscription handling around runtime/native bootstrap,
   transform lifecycle, and initial publish/forwarding ownership.
3. Preserve unsubscribe, disconnect cleanup, bootstrap cancellation, and live
   delivery semantics exactly.

#### Focused verification

- `cargo test -p nimbus-server`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- websocket session code has clearer operational boundaries
- native and runtime subscription bootstrap flows are easier to debug

### OS6. Concept-owned scenario surfaces and idiomatic Rust cleanup sweep

#### Implementation plan

1. Move the highest-value remaining concept-mixed test clusters out of
   `crates/nimbus-engine/src/tests.rs` and `crates/nimbus-storage/src/tests.rs`
   into concept-owned surfaces where it improves maintainability.
2. Keep broad integration coverage intact while reducing the size of the giant
   root test files.
3. Tighten leftover naming, visibility, helper placement, and glue after the
   new production ownership boundaries stabilize.

#### Focused verification

- targeted crate tests for every moved test surface
- `cargo test -p nimbus-engine`
- `cargo test -p nimbus-storage`
- `cargo test -p nimbus-server`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- the remaining giant test roots are materially smaller and more focused
- the codebase reads more canonically after the structural items land

### OS7. Docs and full verification closure

#### Implementation plan

1. Update `ARCHITECTURE.md` if any architecture-level ownership map changed.
2. Update `docs/plans/README.md`, `AGENTS.md`, and any other entrypoint docs if
   plan ownership changes during the workstream.
3. Remove stale checkpoint text and ensure the ledger, dependency graph, and
   execution log match reality.
4. Run the repo-wide verification sweep and record it here.
5. Archive the completed plan once the workstream is fully done.

#### Focused verification

- `make check`
- `make test`
- `make clippy`
- `make ci` if practical

#### Acceptance criteria

- docs reflect the landed architecture and plan ownership
- the full verification sweep is recorded
- the plan can be archived cleanly once the workstream is complete

---

## Execution Log

| Date | Item | Outcome | Summary | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-04 | OS0 | done | Reviewed the live post-cleanup architecture and confirmed that the next high-value cleanup is no longer in the top-level composition roots. The remaining worthwhile work is concentrated in `tenant/mutation.rs`, `store/write.rs`, `evaluator.rs`, `runtime/metrics.rs`, the websocket session/subscription socket surfaces, and the remaining giant engine/storage test roots. Promoted this plan as the next active cleanup control plane. | docs-only planning pass; relied on the previously green deep module-ownership cleanup baseline recorded above | commit the completed previous cleanup handoff plus this new active plan, then start `OS1` |
| 2026-04-04 | OS1 | done | Split `tenant/mutation.rs` into concept-owned modules for queued request models, admission-gate and CoDel ownership, journal queue state plus applied-sequence waiting, public mutation diagnostics snapshot types, and test-only pause control. Kept `tenant.rs` as the stable tenant facade and updated `ARCHITECTURE.md` to reflect the landed mutation subtree. | `cargo fmt --all`; `cargo fmt --all --check`; `CARGO_HOME=/tmp/nimbus-cargo-home cargo test -p nimbus-engine`; `CARGO_HOME=/tmp/nimbus-cargo-home cargo clippy -p nimbus-engine --all-targets -- -D warnings`; attempted `CARGO_HOME=/tmp/nimbus-cargo-home cargo check --workspace` but it is currently blocked by the parallel V8 fork workstream in the same worktree (`rusty_v8` archive download 404 plus outdated `deno eval --allow-net` helper expectations) | start `OS2` by decomposing `store/write.rs` around durable write-path ownership while leaving the separate V8 fork changes untouched |
| 2026-04-04 | OS2 | done | Split `store/write.rs` into concept-owned modules for transaction lifecycle and commit ownership, scheduled-write deduplication plus scheduled-op integration, direct document CRUD helpers, execution-unit batch apply, and `TenantStore` construction or write-entry helpers. Updated `ARCHITECTURE.md` so the storage write subtree reflects the landed ownership map. | `cargo fmt --all`; `cargo fmt --all --check`; `CARGO_HOME=/tmp/nimbus-cargo-home cargo test -p nimbus-storage`; `CARGO_HOME=/tmp/nimbus-cargo-home cargo clippy -p nimbus-storage --all-targets -- -D warnings`; attempted `CARGO_HOME=/tmp/nimbus-cargo-home cargo check --workspace` and confirmed it remains blocked by the unrelated parallel V8 fork workstream (`rusty_v8` archive download 404 plus outdated `deno eval --allow-net` helper expectations) | start `OS3` by decomposing `evaluator.rs` around filter evaluation, ordering, cursor handling, paginated windowing, and shared evaluation helpers while leaving the separate V8 fork changes untouched |
| 2026-04-04 | OS3 | done | Split `evaluator.rs` into concept-owned modules for query surfaces, paginated windowing, shared filter evaluation, ordering validation, and cursor encoding or comparison. Updated `ARCHITECTURE.md` so the evaluator ownership map reflects the landed module tree, and moved the local cursor tests beside the cursor implementation. | `cargo fmt --all`; `cargo fmt --all --check`; `CARGO_HOME=/tmp/nimbus-cargo-home cargo test -p nimbus-engine`; `CARGO_HOME=/tmp/nimbus-cargo-home cargo clippy -p nimbus-engine --all-targets -- -D warnings`; attempted `CARGO_HOME=/tmp/nimbus-cargo-home cargo test -p nimbus-server` but it remains blocked by the unrelated parallel V8 fork workstream when the server build reaches `rusty_v8` (`deno eval --allow-net` incompatibility plus runtime archive download failure) | start `OS4` by decomposing `runtime/metrics.rs` around runtime metrics ownership while leaving the separate V8 fork changes untouched |
| 2026-04-04 | OS4 | blocked | Split `runtime/metrics.rs` in the worktree into a `RuntimeMetrics` facade over concept-owned global-counter, host-operation, tenant-metrics, and request-correlation modules. Formatting is clean, but runtime-focused verification still cannot compile because the parallel V8 fork workstream breaks `rusty_v8` before the runtime crate reaches this code. | `cargo fmt --all`; `cargo fmt --all --check`; attempted runtime or server verification remains blocked by the same parallel V8 fork failure (`rusty_v8` archive download failure and outdated `deno eval --allow-net` helper expectations) | wait for the V8 fork workstream to unblock runtime builds, then resume `OS4`, run the required runtime-focused verification, and only after that decide whether the metrics split can be marked done |
| 2026-04-04 | OS4 | blocked | Confirmed that the temporary `CARGO_HOME=/tmp/nimbus-cargo-home` workaround is no longer needed for the local workspace. Plain `cargo check --workspace` and `cargo test -p nimbus-runtime` now pass, so the old shared-cache or local-Cargo blocker is gone. The remaining blocker is narrower: both `cargo test -p nimbus-server` and `cargo clippy -p nimbus-runtime --all-targets -- -D warnings` fail only when the patched `rusty_v8` fork tries to download `librusty_v8_simdutf_release_aarch64-apple-darwin.a.gz`, which returns `404`, and its downloader still invokes `deno eval --allow-net`, which the installed Deno rejects. | `cargo test -p nimbus-runtime`; `cargo check --workspace`; `cargo clippy -p nimbus-runtime --all-targets -- -D warnings`; `cargo test -p nimbus-server` | keep `OS4` blocked on the separate V8 fork workstream, then rerun the remaining focused verification once the patched `rusty_v8` archive or downloader path is fixed |
| 2026-04-06 | OS4 | done | Resumed `OS4` after the locker fork unblocked plain local Cargo. Kept the metrics split intact, then fixed the runtime-worker affinity regression that was starving same-tenant follow-up work, refreshed the convenience-runtime test to match the default startup-snapshot pool semantics, restored the CLI `RuntimeLimits` initializer with struct update syntax, and moved the delayed async snapshot repro behind a subprocess-backed wrapper so the full runtime suite no longer crashes from prior in-process V8 state. Updated `ARCHITECTURE.md` so the runtime metrics ownership map now names the landed `global.rs`, `host_operations.rs`, `tenants.rs`, and `correlations.rs` submodules. | `cargo fmt --all`; `cargo test -p nimbus-runtime runtime::tests::convenience_runtime_invocations_reuse_runtime_owned_executor -- --exact`; `cargo test -p nimbus-runtime executor::tests::parked_invocation_counts_toward_in_flight_limit -- --exact`; `cargo test -p nimbus-server tests::convex_runtime::cancellation::request_drops::queued::dropped_queued_runtime_request_never_starts_mutation -- --exact`; `cargo test -p nimbus-server tests::convex_runtime::cancellation::request_drops::queued::dropped_queued_runtime_request_recovers_and_serves_new_work_after_pressure_clears -- --exact`; `cargo test -p nimbus-runtime`; `cargo test -p nimbus-server`; `cargo check --workspace`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all --check` | start `OS5` by decomposing the websocket-session and Convex named-subscription hotspots around transport task ownership, pending bootstrap cancellation tracking, registration cleanup, and initial publish or forwarding responsibilities |
| 2026-04-06 | OS5 | done | Split the remaining websocket-session hotspot into `ws/socket/transport.rs`, `ws/socket/pending.rs`, and `ws/socket/session.rs`, then split Convex named-subscription ownership into `named_subscriptions/direct.rs` and `named_subscriptions/runtime.rs` so generic transport tasks, pending bootstrap cancellation tracking, session registration or cleanup, and native versus runtime bootstrap or initial publish flows now have dedicated homes. Updated `ARCHITECTURE.md` to reflect the landed websocket and named-subscription ownership map. | `cargo fmt --all`; `cargo test -p nimbus-server`; `cargo check --workspace`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all --check` | start `OS6` by moving the highest-value remaining engine and storage test clusters to concept-owned surfaces, then tighten naming, visibility, and helper placement around the stabilized module trees |
| 2026-04-06 | OS6 | done | Moved the highest-value remaining concept-mixed tests beside the module trees they now exercise directly: evaluator behavior now lives under `evaluator/tests.rs`, storage scan behavior under `store/scan/tests.rs`, and durable journal stream, recovery, and snapshot rebuild behavior under `store/journal*.rs` test modules. Removed the duplicated root-test copies and stale helpers so the giant engine and storage roots are more integration-oriented. | `cargo fmt --all`; `bash scripts/cargo-isolated.sh -- test -p nimbus-engine`; `bash scripts/cargo-isolated.sh -- test -p nimbus-storage`; `cargo test -p nimbus-server`; `cargo check --workspace`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all --check` | finish `OS7` by running the repo-wide make entrypoints, reconciling any remaining docs or plan ownership updates, and archiving the completed control plane cleanly |
| 2026-04-06 | OS7 | done | Closed the workstream with the repo-level guarded verification entrypoints, moved the completed operational-state cleanup plan out of the active set, and removed the live-plan pointers from `docs/plans/README.md` and `AGENTS.md` so future agents do not resume this finished pass as current progress. The repo-level `make` wrappers initially hit stale single-flight locks from an older local session, but clearing those stale keys unblocked fresh `make check` and `make clippy` runs without any code changes. | `make check`; `make test`; `make clippy`; attempted `make ci` and confirmed it still stops locally because `cargo deny` is not installed | archived plan is the historical record; future cleanup work should start from a newly promoted active plan rather than reviving this one |
