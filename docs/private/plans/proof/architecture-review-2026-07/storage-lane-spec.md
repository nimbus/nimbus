# Storage Lane Spec — SR1 (read-seam capability parameterization), CO6 (mysql/postgres write twins), DS3 (seam truth-up)

Design authority: `architecture-review-2026-07-plan.md` rows + the
2026-07-07 storage inventory. Crate scope: `nimbus-storage` +
`nimbus-engine` (SR1 consumers). SR1 is `large`; CO6 `medium`; DS3
`small` (docs + one latent-bug note). Pre-launch: breaking changes
preferred.

## SR1 — seal the concrete-store leak in the async read seam

### Facts

- `nimbus-storage/src/async_storage/traits.rs`: `TenantReadStorage`
  (:27-53) hands read closures `Arc<Self::Store>` — the leak. The doc
  (:22-26) admits call sites "rely on read tasks receiving
  `Arc<TenantStore>`". `EmbeddedPersistenceProvider` (:16-20) is
  minimal and deliberately keeps planner/journal/snapshot on the
  concrete store.
- The engine consumes NOT `TenantReadStorage` generically but a
  hand-rolled 5-arm enum `TenantPersistence`
  (`nimbus-engine/src/persistence/tenant.rs:19-25`) with
  `delegate_store_method!` fan-out — this is the SR7 enum-dispatch
  (BLOCKED pending owner ADR). SR1 must NOT retire it.
- Existing capability traits (`nimbus-storage/src/traits/core.rs`, the
  MBA2 split): `TenantPointRead` (get), `TenantRangeScan` (7 scan/index
  methods), `DurableJournal` (journal_progress, read_durable_journal_from,
  stream_durable_journal, export/changefeed), `SchedulerStore`,
  `StorageEngine` composite. `traits/query_read.rs` `QueryReadStore`
  (get + the same 6 scan/index methods) is the ONE trait already used
  generically (prepared.rs) and nearly DUPLICATES `TenantRangeScan`.
- Query-closure census (`nimbus-engine/src/engine/queries/`): 7 async
  seam sites (documents.rs:76,344 → `store.get`; prepared.rs:222,250
  already generic over `QueryReadStore`; journal.rs:26;
  materialized.rs:116,194). Deepest leg:
  `tenant/materialized_reads/backend/loading.rs:42-121` calls
  `applied_sequence`, a no-filter `scan_table_matching_cancellable`,
  and `read_commit_log_from` — a snapshot-rebuild surface no trait
  covers.
- Gaps (methods closures call, no trait covers): latest_sequence,
  applied_sequence, read_commit_log_from, recover_durable_journal,
  append/apply_durable_records_batch, export/import_point_in_time_restore_archive,
  read_snapshot(), scan_resource_path_bindings,
  scan_collection_group_bindings, table_id, and the materialized-rebuild
  triad.

### Target (normative) — capability consolidation + read-seam
### parameterization ONLY; enum retirement is SR7 (blocked)

1. Collapse the duplication: make `QueryReadStore` an alias/supertrait
   of `TenantRangeScan + TenantPointRead` (one canonical scan surface).
   If they differ in any method signature, reconcile to one; update the
   `impl_query_read_store!` macro appliers. Net: one read-scan trait,
   not two.
2. Extend `DurableJournal` with the missing sequence/commit-log methods
   it morally owns: `latest_sequence`, `applied_sequence`,
   `read_commit_log_from`, `recover_durable_journal`,
   `append_durable_records_batch`, `apply_durable_records_batch`,
   `export_point_in_time_restore_archive`,
   `import_point_in_time_restore_archive`. (Default-impl where a backend
   legitimately lacks one, else required.)
3. Add two small capability traits: `ResourcePathScan`
   (read_snapshot, scan_resource_path_bindings,
   scan_collection_group_bindings, table_id) and `MaterializedRebuild`
   (applied_sequence + no-filter cancellable table scan +
   read_commit_log_from — the loading.rs triad). Implement all for
   `TenantStore` and the SQL stores via the existing macro pattern.
4. Parameterize the read CLOSURES over the capability-trait sum instead
   of the concrete `Arc<TenantStore>`: change `TenantReadStorage`'s
   `execute`/`execute_cancellable` closure bound so `F` receives
   `Arc<S::Store>` where `S::Store: QueryReadStore + DurableJournal +
   ResourcePathScan + MaterializedRebuild` (or a `ReadCapabilities`
   composite supertrait — prefer one composite for signature sanity).
   The seven query-closure sites now name capability methods, not
   `TenantStore` methods.
5. STOP THERE. The `TenantPersistence` enum stays (SR7 owns its fate);
   `EmbeddedPersistenceProvider::Store` may still be concrete. SR1's
   acceptance is: no `engine/queries/` closure names a `TenantStore`
   method that isn't on a capability trait, proven by the closures
   compiling against a `<S: ReadCapabilities>` bound. Record that
   enum-dyn retirement is deferred to SR7.

## CO6 — split the mysql/postgres write twins

### Facts
`mysql/write.rs` (1,499) / `postgres/write.rs` (1,486). Block A
(store-impl delegation, my:9-419/pg:12-440) ~100% parallel. Block B
(txn-impl, my:421-1453/pg:442-1486) ~22 methods line-for-line parallel
differing only in SQL dialect; 6 are functionally identical
(apply_durable_records_batch, apply_resolved_write, commit, rollback,
record_commit_write, record_tenant_event). Structural divergences that
must NOT be merged blindly: begin/lock order (MySQL SELECT..FOR UPDATE
vs PG pg_advisory_xact_lock), MySQL begin-retry loop, claim_due_jobs
lock mode, PG-only pg_notify path, PG schema-events already extracted
(MySQL still inline at :1455-1492). No shared `sql/` module exists;
row-serde is duplicated (mysql/query_helpers.rs vs postgres/backend.rs).

### Target (normative)
1. PRECONDITIONS first (small, independently valuable): extract MySQL's
   inline schema-event helpers into a `mysql/write_schema_events.rs`
   mirroring the existing PG module; create a shared row-serde module
   `nimbus-storage/src/sql/row.rs` (add `sql` to lib.rs) holding the
   duplicated serialize_json / deserialize_json / serialize_document_fields
   / serialize_document_typed_fields / row_to_document — parameterized
   over a tiny `SqlRow`/`SqlValue` abstraction so both backends call it.
2. `sql/write_core.rs`: a `SqlWriteBackend` trait capturing the dialect
   axes (placeholder style, upsert clause, error mapper, timestamp/seq
   encoding, metadata row shape, `block_on` flavor) + the 6 identical
   txn-orchestration methods implemented ONCE against that trait. Block
   A delegation becomes a shared generic/macro over `SqlWriteBackend`.
3. Per-backend `write.rs` shrinks to: the `SqlWriteBackend` impl
   (statement-building behind a `SqlSession`) + the genuinely divergent
   methods (begin/lock, retry loop, claim_due_jobs, pg_notify). Do NOT
   force the divergent methods into the shared core — they are correct
   as-is and dialect-load-bearing.
4. Acceptance: the 6 identical methods exist once; row-serde exists
   once; the divergent methods remain per-backend with a comment naming
   why. Net line reduction reported. sqlite/libsql untouched.

## DS3 — reconcile storage-seams-architecture.md with shipped code

### Facts + decisions (one per seam; amend spec OR file a bug)
1. Seam B: `ObjectMetaStore` (traits/object_metadata.rs:551) is a
   SYNCHRONOUS journaled capability trait; the spec says async RPITIT.
   DECISION: amend the spec to the shipped sync+journaled shape (it is
   correct and matches the other capability traits).
2. BlobGc (nimbus-blob/src/gc.rs:54): the spec's §9b write-intent pins /
   seal-before-enumerate / seam-generic sweep are NOT implemented — only
   a wall-clock grace_window protects in-flight writes; `sweep()` is
   enumerate-then-release (TOCTOU). AND `nimbus-sandbox/src/volume.rs:271`
   puts a snapshot blob that is not pinned/registered as a GC root
   between `store.put` and recording the SnapshotId — the exact
   unpinned-write case. DECISION: this is a LATENT CORRECTNESS GAP, not
   just doc drift. File it as a new GR-band-style row
   (`GR9: BlobGc write-intent pins`) in the ledger with the volume.rs
   snapshot-put as the concrete repro, and amend §9b to describe the
   grace-window-only state as current + pins as planned. Do NOT
   implement the pin registry in this DS3 slice (it is a real design
   task — scope it, don't smuggle it).
3. Seam E: `VolumeProvider` EXISTS (volume.rs:117, LocalDirVolume :149)
   matching spec §16b — the first-pass "absent" finding was wrong.
   Amend the spec's §16b/§16c: confirm VolumeProvider shipped; truth-up
   §16c crate inventory (blob/fs/s3 shipped; object plane crate is
   `nimbus-object-storage` not `nimbus-objectfs`; NOS/NFS/NC archived)
   and §12 topology.
4. Remaining Seam E work (parity, GC/placement wiring, more backends)
   is owned by `nimbus-sandbox-plan.md` — cross-reference, don't
   duplicate.

## Hard constraints

- Storage atomicity invariant untouched: doc write + index effects +
  commit-log append stay one transaction on every path CO6 refactors.
- SR1 must not retire the `TenantPersistence` enum (SR7, blocked) and
  must not change any query result — it is a type-surface consolidation
  proven by the closures compiling against capability bounds + the full
  engine suite staying green.
- CO6 must not merge the dialect-divergent methods; the SQL backends'
  observable behavior (lock order, notify, retry) is unchanged.
- DS3 implements NO new runtime behavior (the BlobGc pin registry is
  scoped out to GR9).

## Verification gates (worktree root, report real counts)

```
cargo fmt --all --check
cargo clippy -p nimbus-storage -p nimbus-engine --all-targets -- -D warnings
cargo test -p nimbus-storage
cargo test -p nimbus-engine
cargo test -p nimbus-system
cargo check -p nimbus-server
```

SR1 touches the read path broadly — full engine + storage suites, not
filtered subsets. CO6's mysql/postgres suites are often CI-gated
(external DB); run what runs locally and say which skipped (CI owns
them). Update ledger rows SR1/CO6/DS3 (+ add GR9) with evidence.
