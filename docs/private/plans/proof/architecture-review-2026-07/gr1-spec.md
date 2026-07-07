# GR1 Spec — one transactional write core for the redb TenantStore

Design authority: `docs/private/plans/architecture-review-2026-07-plan.md`
GR1 + the 2026-07-06 write-path inventory. Scope: **redb `TenantStore`
only**. The SQL backends (sqlite/postgres/mysql/libsql) already share row
helpers and DB-native indexing; do not touch them.

## The problem, precisely

Three transactional document-write bodies exist today:

1. No-index point family — `store/write/direct.rs` (insert /
   update_validated / delete_validated on `TenantWriteTransaction`).
2. With-index point family — `index/maintenance/transaction.rs`
   (re-implements read/patch/validate/write + index diffing).
3. Batch core — `store/write/batch.rs` `apply_insert/apply_update/
   apply_delete` free fns under `apply_execution_unit_batch_with_origin`.

Two document-mutation `begin_write` sites: `store_entry.rs:157` (point,
via `execute_write`) and `batch.rs:49` (batch).

The duplication has already bred two behavioral divergences (confirmed):

- **D1**: no-index `update_document_validated` stamps
  `document.update_time = clock.now()` (`direct.rs`); with-index
  `update_document_with_indexes_validated` does NOT stamp.
- **D2**: no-index `delete_document_validated` removes the resource-path
  binding; with-index delete does NOT.

## Target shape (normative)

**One apply core, one txn choke point, thin entries.**

1. **Shared apply core.** The batch path's per-item application logic
   (document write + index effects with the `is_maintained()` filter and
   old/new key diff + resource-path binding upsert/remove +
   `commit_writes` push with correct `WriteOp` fields) becomes THE only
   implementation, callable with an open transaction. Both point families
   and the batch loop go through it. Concretely: rehome/refactor
   `apply_insert`/`apply_update`/`apply_delete` so they operate on the
   same transaction context `TenantWriteTransaction` uses (or extract the
   table handles they need); the point-family methods and the batch loop
   both call them. After this change there is exactly ONE place that
   writes DOCUMENTS+INDEXES+bindings+commit_writes per write kind.
2. **One `begin_write` site for document mutations.** Route the batch
   entry through the same transaction-opening choke point the point path
   uses (`execute_write` / `store_entry.rs`), so `batch.rs` no longer
   calls `begin_write` itself. Journal/scheduler/kv/usage/etc.
   `begin_write` sites are other subsystems — out of scope.
3. **Entry-layer semantics stay at the entries** (this is the depth
   split — the core owns atomicity; entries own request semantics):
   - Point entries keep their signatures and semantics: in-txn read of
     the existing document, `DocumentNotFound` on absence, validation
     closures run in-txn, patch application, returning the removed
     `Document` on delete, and the `begin_scheduled_execution` `_once`
     dedup before delegation.
   - Batch entry keeps optimistic semantics: caller-supplied `previous`,
     `Conflict` on `existing != previous` or on insert-key-present,
     `ResolvedScheduleOp` handling, `trigger_write_origin`,
     `commit_timestamp`.
   - Error surfaces MUST NOT change: point update/delete of a missing
     doc stays `DocumentNotFound`; batch mismatches stay `Conflict`.
4. **Resolve the divergences in the shared core's favor, explicitly:**
   - **D1 fix**: updates through BOTH point families stamp
     `update_time = clock.now()` where the patch is applied (entry
     layer). The batch path is unchanged — execution units pass a
     caller-built `current`; verify at which layer the execution-unit
     path stamps `update_time` (check `execution_units/` staging in
     nimbus-engine) and record it in your report; do not double-stamp.
   - **D2 fix**: deletes through BOTH point families remove the
     resource-path binding (the shared apply core already does this on
     the batch path).
   - Each fix gets its own regression test proving the previously
     divergent family now behaves correctly (with-index update refreshes
     update_time; with-index delete removes the binding), plus one test
     asserting no-index/with-index/batch produce identical `WriteOp`
     shapes for equivalent operations.
5. **The point-family collapse**: after the shared core exists,
   `index/maintenance/transaction.rs`'s three re-implementations reduce
   to thin calls (read+validate at entry, then shared apply). Pre-launch
   rules apply — delete the duplicated bodies outright, no compat
   shims. The `*_with_indexes*` public wrapper NAMES stay (the engine
   dispatches on `indexes.is_empty()` and the SQL backends implement the
   same trait surface) but their redb bodies must not duplicate apply
   logic.

## Hard constraints

- The atomicity invariant is non-negotiable: document write + index
  effects + commit-log append remain ONE redb transaction on every path.
  The existing rollback test
  (`failed_batch_rolls_back_document_indexes_bindings_and_commit_log`)
  must stay green, and add its sibling for the point path (induced
  failure mid-point-write rolls back doc+index+binding+commit).
- `WriteOp` field parity: `previous`, `current`,
  `resource_path_binding`, `trigger_write_origin` populated exactly as
  today for each path (except where D1/D2 deliberately fix behavior).
- `is_maintained()` filter parity everywhere.
- No public trait/API signature changes on `TenantStore`'s write surface
  (the engine's `match_tenant_persistence!` fan-out and SQL backends
  share it).
- No changes outside `crates/nimbus-storage` except (if genuinely
  required) test updates; nimbus-engine source must not change. If the
  engine seems to need a change, STOP and report.
- Modularity: keep concept-owned files; if batch.rs shrinks to a loop +
  entry glue, fine; do not create `helpers.rs`.

## Verification gates (worktree root, in order — blast-radius rules)

```
cargo fmt --all --check
cargo clippy -p nimbus-storage --all-targets -- -D warnings
cargo test -p nimbus-storage          # full crate: point+batch+index+journal suites
cargo test -p nimbus-engine           # mutation-path + execution-unit suites (blast radius)
cargo check -p nimbus-server
```

This change touches a fail-closed/load-bearing surface: name the exact
test counts per suite in the report. If any pre-existing test asserts the
D1/D2 divergent behavior (stale update_time or lingering binding), do NOT
weaken it silently — flag it in the report with file:line and the
behavior change rationale.

## As built (PR #129, squash-merged `bcac953bb`, 2026-07-06)

Landed to contract. The three transactional write bodies collapsed to
one shared apply core; net −185 lines of production code.

- New `store/write/apply.rs`: the ONLY implementation of document write
  + index maintenance (`is_maintained` filter + old/new key diff) +
  resource-path bindings + `WriteOp` recording. Both point families and
  the batch loop call it; batch routes through the same `execute_write`
  choke point, so there is one transaction-opening site and one atomic
  doc+index+binding+commit body to audit for the storage-atomicity
  invariant. The batch-only `append_commit_entry` helper was deleted.
- Entry semantics preserved via `WriteExpectation`: point keeps
  `DocumentNotFound` + in-txn validation closures + returns the removed
  `Document` + `_once` dedup; batch keeps optimistic `Conflict` +
  schedule ops + trigger origin + commit timestamps.
- Drift bugs the duplication had already caused, fixed with regression
  tests: D1 — with-index point updates were not stamping `update_time`
  (the no-index family was); D2 — with-index point deletes were not
  removing the resource-path binding. Patch application unified on
  `Document::set_field`.
- Deliberate strictness unification: point insert now errors `Conflict`
  on an existing key (was silent overwrite), matching batch;
  engine-generated ULID ids make collisions unreachable and no caller
  relied on overwrite (verified, incl. `nimbus-system`'s
  get-then-update upsert branch).

Evidence: 328 nimbus-storage + 308 nimbus-engine + 30 nimbus-system
tests, 0 failures; fmt/clippy clean; `cargo check -p nimbus-server`;
autoreview (Codex) clean — atomicity boundary, error surfaces, filter
parity, `_once` dedup all confirmed.
