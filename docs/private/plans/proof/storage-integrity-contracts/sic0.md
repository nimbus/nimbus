# SIC0 — Baseline, writer census, and fail-before evidence

Execution baseline: `main` @ `49884476dbf6f31a0c003f580d070cb6734e9a93`
(plan activation commit; production tree identical to
`8877eaff43a36d9606a1feaa0ab31d0377539d9d`).

Machine: darwin 24.6.0, aarch64, rustc stable toolchain.
No production source changed in this task. `git diff -- crates packages` is
empty.

## 1. Red verifier baseline

Command:

```bash
bash docs/private/plans/proof/storage-integrity-contracts/verify.sh
```

Result: exit status `1`, `Summary: 0 passed, 13 failed`.

| # | Condition | Result | Reason |
|---|---|---|---|
| 1 | typed object condition types | FAIL | `ObjectExpectedState`/`ObjectConditionOutcome` absent from `crates/nimbus-storage/src/traits/object_metadata.rs` |
| 2 | condition crosses `S3ObjectMeta` | FAIL | `put_manifest_conditional` absent from `crates/nimbus-s3/src/backend.rs`; `crates/nimbus-s3/src/service.rs` still calls `verify_write_preconditions` |
| 3 | actor decides before sequencing | FAIL | `evaluate_object_condition` absent from `crates/nimbus-engine/src/engine/objects.rs` |
| 4 | sequential + concurrent conditional probes | FAIL | neither named test exists |
| 5 | rejection has no commit or blob effect | FAIL | named test does not exist |
| 6 | concurrent multipart writes preserve every accepted part | FAIL | named test does not exist |
| 7 | all storage writers are inventoried | FAIL | named test does not exist |
| 8 | an omitted effect fails the ownership gate | FAIL | named test does not exist |
| 9 | one canonical digest | FAIL | `MaterializedPosition` absent; the engine canonicalizer still exists |
| 10 | divergence + ordering tests | FAIL | neither named test exists |
| 11 | materialized consumers bind the position | FAIL | neither named test exists |
| 12 | provider qualification matrix is complete | FAIL | named test does not exist (`--features libsql,mysql,postgres`) |
| 13 | physical SQLite durability faults | FAIL | 0 of 4 named cases exist |

The verifier is non-vacuous by construction. Source conditions name symbols that
do not exist yet. Every test condition requires exit status `0` **and** at least
one line matching `^test .*<filter>.* ... ok`, so a filter that selects zero
tests fails instead of passing silently. `SIC_SKIP_TESTS=1` reports every test
condition as failed and exists only for inspection; it is never campaign
evidence.

Raw output: `sic0-baseline.txt`.

## 2. Client mutation route census

Repository instructions permit exactly three client document mutation routes.
All three reach the shared SQL core and construct `SqlCommitEffects`.

| Route | Engine entry | Shared SQL commit plan |
|---|---|---|
| Queued journal | per-tenant committer batches, `crates/nimbus-engine/src/engine/mutations/publisher.rs` | `crates/nimbus-storage/src/sql/store_core.rs:262` `apply_prepared_write_batch`, effects at `:277` |
| Queued journal, fenced | same, under a held committer lease | `crates/nimbus-storage/src/sql/store_core.rs:292` `fenced_apply_prepared_write_batch`, effects at `:314` |
| Execution unit | `crates/nimbus-engine/src/engine/execution_units/mod.rs:28` `MutationExecutionUnit` | `crates/nimbus-storage/src/sql/store_core.rs:702` `apply_execution_unit_batch_with_origin`, effects at `:719` |
| Direct | `crates/nimbus-engine/src/engine/mutations/direct/execution.rs:31` `apply_mutation_with_mode` | **no `SqlCommitEffects` witness** — reaches `TenantPointWrite` per-operation validators |

The U5 witness (`crates/nimbus-storage/src/sql/commit_effects.rs:167`
`SqlCommitEffects`, eight fields, no `Default`, no `Option`) forces the three
composite construction sites to name every effect. The direct path is
deliberately excluded by the U8 decision; the module doc at
`crates/nimbus-storage/src/sql/commit_effects.rs` states the gap: "adding a
field here does not force the direct path to declare a position on it." This is
finding F3 and belongs to SIC3.

## 3. Non-client storage writer census

These writers are not client document mutations. They must still be inventoried
because SIC3 makes cross-cutting effects explicit on every writer.

| Writer | Entry point | Note |
|---|---|---|
| Schema replace/delete | `crates/nimbus-engine/src/engine/schema.rs` → `crates/nimbus-storage/src/sql/store_core.rs:448-499` (`replace_table_schema`, `fenced_replace_table_schema`, `delete_table_schema`, `fenced_delete_table_schema`) | fenced variants validate `(owner_id, epoch, expected head)` in the same transaction |
| Scheduler jobs, cron, results, recovery | `crates/nimbus-engine/src/engine/scheduler/access.rs`; `crates/nimbus-storage/src/scheduler/{jobs,cron,results,recovery}.rs`; `crates/nimbus-storage/src/sql/store_core.rs:571-613` | allocates no journal sequence; schedule-only execution units still fence atomically |
| Trigger candidates and invocation lifecycle | `crates/nimbus-engine/src/tenant/trigger_candidates.rs`; `crates/nimbus-storage/src/store/trigger_invocations.rs`; `crates/nimbus-storage/src/store/trigger_delivery.rs`; `crates/nimbus-storage/src/sql/store_core.rs:662,676` | idempotent complete-record replacement, serialized through the committer |
| Committer lease acquire/renew | `crates/nimbus-engine/src/tenant/committer_lease.rs` | provider-owned expiry and epoch |
| Journal maintenance and retention GC | `crates/nimbus-engine/src/engine/queries/journal.rs`; `crates/nimbus-storage/src/sql/store_core.rs:351` `compact_retained_versions` | asserts `commit.is_none()` |
| PITR export/import | `crates/nimbus-storage/src/sql/store_core.rs:368` `export_...`, `:384` `import_...`, `:408` `fenced_import_...` | compares restored fingerprint against `archive.target_fingerprint` |
| Object metadata (manifests, multipart) | `crates/nimbus-engine/src/engine/objects.rs:248` `commit_meta_write` → `:294` `commit_object_meta_write_in_actor` via `crates/nimbus-engine/src/tenant/mutation_facade.rs:330` `submit_internal_committer_async` | **internal** route, not a fourth client route |
| Table lifecycle | `crates/nimbus-storage/src/store/table_lifecycle.rs` plus per-provider `*/table_lifecycle.rs` | retention-floor gated |
| Resource-path projections | `crates/nimbus-storage/src/store/resource_paths.rs` plus per-provider modules | |
| libSQL replica cache materialization | `crates/nimbus-storage/src/libsql/provider.rs:551-565` `materialize_snapshot_to_replica_cache`; refresh state at `crates/nimbus-storage/src/libsql/freshness.rs` | local replica cache only; not an authoritative writer |
| Cross-tenant usage/control | `crates/nimbus-storage/src/usage_store.rs` | separate local redb control database |

### Correction to a stale repository note

`CLAUDE.md` states that "Object manifests use the raw `TenantPointWrite` seam on
the read executor." That is no longer true for production. The four
`*_direct` helpers in `crates/nimbus-storage/src/traits/object_metadata.rs`
(`put_object_manifest_direct:750`, `delete_object_manifest_direct:786`,
`put_multipart_upload_direct:832`, `delete_multipart_upload_direct:867`) are all
`#[cfg(test)]`. Production object metadata publishes only through
`commit_object_meta_write_in_actor`. SIC7 reconciles the note.

### Source-level writer gates that already exist

`crates/nimbus-storage/src/tests/commit_path_ownership.rs` scans the provider
source trees and pins fault-point ownership:

- `u4_commit_sequence_fault_points_live_only_in_the_shared_sql_core` (`:117`)
- `u4_journal_fault_points_stay_with_their_pinned_owners` (`:171`)
- `assert_scan_set_is_intact` (`:98`) requires at least 45 scanned files, so the
  scan cannot silently shrink.

This file is the natural home for SIC3 conditions 7 and 8.

## 4. Fingerprint producer and consumer census

Two independent canonicalizers exist today. This is finding F4 and belongs to
SIC4.

### Producers

| Producer | Path | Scope |
|---|---|---|
| `MaterializedJournalSnapshot::canonical_fingerprint` | `crates/nimbus-storage/src/store/journal_snapshot.rs:174` | storage-owned, provider-independent |
| `materialized_snapshot_fingerprint_after_rebuild` | `crates/nimbus-storage/src/store/journal_snapshot.rs:595` | PITR archive target |
| `snapshot_fingerprint` | `crates/nimbus-engine/src/verification.rs:74` | engine duplicate |
| `bootstrap_fingerprint` | `crates/nimbus-engine/src/verification.rs:91` | engine duplicate, embeds the snapshot digest |
| `canonicalize_materialized_journal_snapshot` | `crates/nimbus-engine/src/verification.rs:335` | engine duplicate canonicalizer; helpers at `:351`, `:380`, `:386`, `:392` |

### Consumers

| Consumer | Path | Binds |
|---|---|---|
| PITR import, shared SQL core | `crates/nimbus-storage/src/sql/store_core.rs:395`, `:432` | sequence + digest |
| PITR import, SQLite | `crates/nimbus-storage/src/sqlite/journal.rs:219` | sequence + digest |
| PITR import, memory | `crates/nimbus-storage/src/memory/journal.rs:568` | sequence + digest |
| PITR import, store facade | `crates/nimbus-storage/src/store/journal_snapshot.rs:446` | sequence + digest |
| PITR archive build | `crates/nimbus-storage/src/store/journal_snapshot.rs:552`, `:608` | sequence + digest |
| Engine verification query | `crates/nimbus-engine/src/engine/queries/verification.rs:84-87` | authoritative, shadow, embedded replica, bootstrap |
| Shadow materializer manifest | `crates/nimbus-storage/src/materializer/mod.rs:40` `ShadowMaterializerManifest`, `validate` at `:50` | **sequence only** — `:67` compares `checkpoint_sequence != checkpoint.applied_sequence` and never compares a digest |

The shadow gap is exactly what SIC4's
`shadow_recovery_rejects_wrong_checkpoint_digest` must close.

## 5. Fail-before evidence

Both probes were appended to `crates/nimbus-s3/src/tests.rs` in a temporary
module, run, and then removed by restoring a saved copy taken before the edit
(`sha256 d41ff1448590d0d810f0a03580c82ac2f9d9768bdf74143950700c834d9608d9`,
verified equal after restore). `git checkout --` was not used.

The probes use a `GatedMeta` decorator over the existing in-memory
`S3ObjectMeta`. It releases each read only after both concurrent requests have
read, which reproduces deterministically the read/read/write/write interleaving
that two server tasks can produce against one logical key. No production code
was involved in the gate.

Command:

```bash
cargo test -q -p nimbus-s3 sic0_probe -- --nocapture
```

Output:

```text
running 2 tests
SIC0 PROBE conditional-put: first_accepted=true second_accepted=true
SIC0 PROBE multipart: part1_accepted=true part2_accepted=true durable_parts=[1]
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 20 filtered out
```

### F1 — conditional `PutObject` is not linearizable

Two concurrent `PutObject` requests with `If-None-Match: *` at the same bucket
and key were **both accepted**. S3 requires at most one to succeed.
`crates/nimbus-s3/src/service.rs` reads the manifest, evaluates
`verify_write_preconditions` against that stale read, writes the blob, and then
calls the unconditional `put_manifest`. The condition is decided outside the
authority that serializes the write. This is the fail-before state for
`conditional_put_if_none_match_is_linearizable` (verifier condition 4).

### F2 — concurrent `UploadPart` loses an accepted part

Two concurrent `UploadPart` requests for part numbers 1 and 2 of one upload were
**both accepted**, but the durable upload record retained only part `1`. Part 2
was acknowledged to the client and then lost. `upload_part` reads the upload
record, calls `ObjectMultipartUpload::replace_part` on its local copy, and
writes the whole record back unconditionally. No S3 conditional header exists on
`UploadPart`, so no wire-level policy can compensate. This is the fail-before
state for `concurrent_upload_parts_preserve_all_accepted_parts` (verifier
condition 6).

Raw output: `sic0-failbefore.txt`.

### Blob-cleanup hazard to carry into SIC1 and SIC2

When the loser and the winner upload identical bytes, both resolve to the same
content hash. `release_blob_unless_manifest_retains` and
`release_manifest_blobs_except` in `crates/nimbus-s3/src/service.rs` key cleanup
on the loser's stale `previous` manifest, so a rejected write can delete the
blob the winner retains. Invariant 6 requires a rejected condition to have no
retained-blob effect; it equally requires no cleanup effect on the winner.

## 6. Provider lane status at baseline

| Lane | Status |
|---|---|
| Embedded SQLite / memory / redb | ran locally |
| libSQL, MySQL, PostgreSQL | `--features libsql,mysql,postgres` compiled; no live fixture on this host, so provider-specific cases are **UNVERIFIED**, not green |

Invariant 12 applies. SIC5 owns the complete qualification matrix.
