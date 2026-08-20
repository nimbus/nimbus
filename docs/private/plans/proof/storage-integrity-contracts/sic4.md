# SIC4 — One canonical materialized position

Base: `main` @ `1f1ccce65742b5ba33da61db9f9b0a4f779f4fa5` (SIC3 merged).
Machine: darwin 24.6.0, aarch64, rustc 1.96.1.

## 1. The gap

Finding F4: storage and engine each owned a snapshot canonicalizer, and the
persisted shadow checkpoint bound only a sequence.

The duplication was not cosmetic. The two canonicalizers disagreed on one
field, and that disagreement was a live defect rather than a latent one.
`Schema::tables` is a `HashMap`. The engine canonicalizer sorted it into a
`Vec<TableSchema>` before hashing; storage's `canonical_fingerprint` serialized
the map directly. Every `HashMap` carries its own `RandomState`, so the storage
digest of one logical state was different in every instance that built it.

Point-in-time restore compares exactly that digest. `build_point_in_time_
restore_archive` hashed the target state through one `Schema` instance and
`import_point_in_time_restore_archive` hashed the restored state through
another, so a multi-table tenant could not restore at all.

## 2. Fail-before

`sic4-failbefore.txt`. Three temporary tests appended to
`crates/nimbus-storage/src/tests/recovery.rs`, run, then removed byte-exactly.

**A — the plan's fail-before.** Alter checkpoint content without moving its
sequence, then recover:

```
FAILBEFORE-A: recover() ACCEPTED the tampered checkpoint;
recovered title = Some(String("tampered"))
```

`ShadowMaterializerManifest::validate` compared `checkpoint_sequence !=
checkpoint.applied_sequence` and nothing else, so a materializer recovered
state that never came from the journal and reported success.

**B — the digest was not a function of the state.**

```
FAILBEFORE-B: 200 logically identical snapshots produced 101 distinct
canonical_fingerprint values
```

**C — the consequence, worse than the plan anticipated.**

```
FAILBEFORE-C: 40/40 point-in-time restores of a 5-table tenant failed;
last error: storage error [corruption]: point-in-time restore fingerprint
mismatch: restored 1f905169… expected 675c5d30…
```

PITR restore was broken for any tenant with more than one table. The single-
table PITR tests passed because a one-entry `HashMap` has one iteration order.

## 3. The contract

`crates/nimbus-storage/src/store/journal_snapshot.rs` owns two new types and
storage is now the only crate that hashes a snapshot.

```rust
pub struct CanonicalMaterializedState {
    pub snapshot_version: u16,
    pub table_identities: Vec<TableIdentitySnapshotEntry>,
    pub schema_tables: Vec<TableSchema>,
    pub documents: Vec<Document>,
    pub scheduled_execution_ids: Vec<String>,
}

pub struct MaterializedPosition {
    pub version: u16,
    pub applied_sequence: SequenceNumber,
    pub state_digest: String,
}
```

**What the digest covers.** `CanonicalMaterializedState` sorts all four
collections — identities by (namespace, table, table_id, state), schema tables
by name, documents by (table, id), scheduled execution ids lexically — and the
digest is SHA-256 over its JSON. `serde_json`'s `preserve_order` is off
workspace-wide, so a document's own field map is a `BTreeMap` and already
canonical; the schema map was the only unordered input, and sorting it is what
makes the digest a function of the state.

**What the digest does not cover, and why.** Sequences are absent from
`CanonicalMaterializedState`: it answers *what state*, and `MaterializedPosition`
pairs it with *how far*. Durable head is absent from both the state and the
position, per plan step 1 — it is a durability fact about the journal, not a
property of the state the journal produced. A snapshot and a replica of it can
legitimately sit at the same position with different durable heads, and the
consistency verifier compares durable head as its own field so a real
divergence is still named.

`version` on the position is the digest-format version. A stale digest cannot
compare equal to a fresh one across a future layout change, because the version
is part of the compared value.

## 4. Consumers bound to the position

| Consumer | Was | Is |
|---|---|---|
| `PointInTimeRestoreArchive` | `target_fingerprint: String` | `target_position: MaterializedPosition` |
| PITR import — redb, memory, sqlite, and both SQL routes | compares a bare digest string | compares the position |
| `ShadowMaterializerManifest` | `checkpoint_sequence: SequenceNumber` | `checkpoint_position: MaterializedPosition` |
| `SnapshotFingerprint` | `digest`, `applied_sequence` | `position`, plus `snapshot_version` |
| `BootstrapFingerprint` | `snapshot_digest` | `snapshot_position` |
| Engine snapshot comparison | its own canonicalizer | `MaterializedJournalSnapshot::canonical_state` |

The engine's `canonicalize_materialized_journal_snapshot`,
`CanonicalMaterializedJournalSnapshot`, `CanonicalTableIdentity`, and the four
`canonicalize_*` helpers are deleted. `crates/nimbus-engine/src/verification.rs`
no longer imports `sha2`.

`compare_materialized_journal_snapshots` now decides equality by position and
returns `Result<Option<ConsistencyMismatch>>`. The field walk survives as
`locate_canonical_state_difference`, but its job changed: it no longer decides
whether the snapshots agree, only *where* a digest difference came from, so an
operator still gets `documents.tasks/<id>` rather than two opaque hashes. If the
walk finds nothing while the digests differ, the mismatch is reported against
`position.state_digest` instead of silently returning "equal" — the walk cannot
turn a real divergence into a green report.

`ShadowMaterializerManifest::validate` compares the full position, which is what
closes fail-before A.

## 5. Clean break, no shim

Plan step 5. `PointInTimeRestoreArchive` and `ShadowMaterializerManifest`
changed shape with no legacy decoding path, and the `/debug/tenants/{id}/
consistency` report changed shape with it. `docs/operators/observability.md` is
updated to the new fields. `MATERIALIZED_JOURNAL_SNAPSHOT_VERSION` is now `pub`
so a consumer can name the version it builds instead of hard-coding an integer
that silently rots.

## 6. Verification

| Command | Result |
|---|---|
| `cargo test -p nimbus-storage materialized_position -- --nocapture` | 2 passed |
| `cargo test -p nimbus-storage pitr_import_rejects_wrong_target_digest -- --nocapture` | 1 passed |
| `cargo test -p nimbus-engine shadow_recovery_rejects_wrong_checkpoint_digest -- --nocapture` | 1 passed |
| `cargo test -p nimbus-storage journal_snapshot -- --nocapture` | see `sic4-verify.txt` |
| `cargo test -p nimbus-engine verification -- --nocapture` | vacuous: matches only the two ignored harness cases; see `sic4-verify.txt` |
| `cargo test -p nimbus-engine consistency` | 20 passed; this is the filter that actually exercises `verification.rs` |
| plan verifier | conditions 1–11 green; see `sic4-verifier.txt` |
| `make ci` | `MAKE_RC=0`, zero FAIL lines; lanes in `sic4-ci-lanes.txt` |

The four acceptance tests:

- `same_sequence_different_state_has_different_materialized_position` — equal
  applied sequence, unequal state, unequal position. The test asserts the
  sequences stayed equal first, so it cannot pass by accidentally moving the
  sequence.
- `logical_order_does_not_change_materialized_position` — the same parts
  assembled in opposite order share one position, and 32 fresh instances of the
  same logical state produce exactly one digest. That second assertion is the
  direct inverse of fail-before B, which saw 101 distinct values over 200.
- `pitr_import_rejects_wrong_target_digest` — the archive restores honestly
  first, then only `target_position.state_digest` is corrupted. Target sequence,
  base snapshot, and journal tail all still agree, so a sequence-only target
  would accept it.
- `shadow_recovery_rejects_wrong_checkpoint_digest` — recovery against the
  honest checkpoint succeeds, then the same manifest is offered a checkpoint
  whose content changed at an unchanged sequence and is rejected.

Provider position parity is covered by
`canonical_digest_generated_history_matches_redb_sqlite_pitr_cdc_and_rebuild_paths`,
which asserts redb and SQLite produce the same `materialized_position` for
generated histories and the same `target_position` for their PITR archives.

## 7. Fixtures the validating digest corrected

`canonical_state` validates before it hashes, so a digest is only defined over a
well-formed snapshot. Three existing fixtures were building snapshots that no
storage path can produce, and the validation surfaced them:

- `nimbus-engine snapshot_comparison_reports_table_identity_state_drift` set
  `state: Deleting` while leaving `namespace: "default"`. A snapshot's namespace
  is derived from the state and table id, so that pair cannot occur. The fixture
  now moves the namespace with the state, which is the real drift shape, and the
  verifier reports it on the `table_identities` key set rather than on one
  paired entry. Per-identity path coverage is unchanged and still proven by
  `snapshot_comparison_reports_table_identity_table_id_drift`, whose two sides
  stay valid.
- `nimbus-object-storage backup_roots_are_extracted_from_object_manifest_snapshot`
  pinned `version: 0` and pushed a manifest document into `_nimbus_objects` with
  no table identity. It now names
  `MATERIALIZED_JOURNAL_SNAPSHOT_VERSION` and carries the identity its document
  requires.
- `nimbus-engine materialized_snapshot_with_documents` pinned `version: 2`
  against a current version of 3.

No assertion was weakened to reach green. Each fixture moved toward the shape
storage actually writes.

`nimbus-storage redb_storage_engine_quality_performance_budget_covers_latest_historical_cdc_pitr_and_gc`
failed its 1s PITR export/import bound at 1.160s inside the 1156-test
concurrent run and passes at 0.273s run alone — a 4.3x contention factor, the
same wall-clock class as blocker B2. The position costs one `serde_json` pass
and one SHA-256 over the canonical state at export and one at import, which is
the same shape and count of work the deleted `canonical_fingerprint` did.
