# SMR0 Baseline And Contract Verifier

Status: `done` on work commit; plan transition waits for merge.

Baseline: `3054743c5` (`cc7ae36a3` is the pre-promotion code baseline; the
promotion commit changed private plans only).

## Result

SMR0 establishes the exact boundary for SA4 without changing production
behavior. Nimbus already has seven parts of the retention contract: typed
resource watermarks, participant routing, atomic MVCC compaction, document
anchor preservation, closed index-interval pruning, typed expired-history
errors, diagnostics, provider floor accessors, and optimistic journal cursor
checks. Eleven conditions are red: four explicit shipped windows, a durable
materialized checkpoint, desired/confirmed/physical floors, journal pruning,
nonzero PITR bases, embedded crash/restart proof, provider lease fencing,
Engine lifecycle ownership, post-page validation, lifecycle observability, and
closeout evidence.

## Source Inventory

| Contract | Current owner | Baseline state |
| --- | --- | --- |
| Resource watermarks and pins | `crates/nimbus-storage/src/retention.rs` | Implemented and unit-tested. One sequence window feeds every resource. Pins are process-local and have no production callers. |
| Document versions | redb, SQLite, PostgreSQL, MySQL, and libSQL document-version modules | `compact_retained_versions` preserves the newest anchor at or before the floor. No production caller. |
| Index versions | redb, SQLite, PostgreSQL, MySQL, and libSQL index-version modules | Closed intervals with `visible_until <= floor` are pruned. No production caller. |
| Durable journal | backend journal/read modules | Append, replay, bootstrap, and page reads exist. Physical removal exists only as `MemoryTenantStore::prune_durable_journal_through_for_testing`. |
| Cursor floor | redb/SQLite/PostgreSQL/MySQL/libSQL journal readers | Inferred as `oldest physical row - 1`. It remains 0 because production never deletes rows. |
| PITR | `store/journal_snapshot.rs` plus provider wrappers | Export uses an empty sequence-0 base and reads from sequence 1. Import rejects every nonempty base. |
| Changefeed | `changefeed.rs` and durable journal pages | Behind-floor errors map to `RetentionExpired`; page implementations check before reading. Remote providers do not revalidate after the row query. |
| Engine lifecycle | Engine tenant runtime, background executors, and internal committer route | The lifecycle has no retention controller. Engine uses `RetentionGcConfig::default()` only for latest PITR export, which means retain all. |
| Diagnostics | `crates/nimbus-storage/src/diagnostics.rs` | Exposes MVCC watermarks and active pins. It has no checkpoint or maintenance outcome. |
| Authority | SA3 process fence and provider committer lease | The required primitives exist. No retention finalization method consumes them. |

## Verifier Fail-Before

Command:

```text
bash scripts/verify-storage-metadata-retention.sh
```

Result: exit 1.

```text
PASS: resource-specific watermarks and participant routing exist
PASS: MVCC compaction exists on embedded and SQL storage seams
PASS: existing tests preserve document anchors and closed index intervals
PASS: trimmed cursor and PITR errors already have a typed classification
PASS: storage diagnostics expose current MVCC watermarks and pins
PASS: all provider stores expose the current retention-floor seam
PASS: journal pages perform an optimistic retention-floor check
FAIL: four durable windows and a shipped Engine profile are explicit
FAIL: a materialized retention checkpoint binds the retained replay base
FAIL: desired, confirmed, and physical floors are distinct state
FAIL: one maintenance contract checkpoints and prunes journal plus MVCC history
FAIL: PITR export and import accept a validated nonzero retained base
FAIL: memory, redb, and SQLite prove restart and checkpoint fault atomicity
FAIL: provider retention finalization is lease-fenced and tested
FAIL: the Engine lifecycle runs bounded maintenance and exposes retain-all explicitly
FAIL: paged consumers revalidate after reads and cover concurrent pruning
FAIL: bounded metrics and diagnostics expose lifecycle outcomes and floor lag
FAIL: closeout proof records a green verifier, repository gate, review, and SAFE verdict

Summary: 7 passed, 11 failed
```

## Capacity Calibration

The prior SEQ13 smoke case covered 80 document commits. It reported PITR
export/import in 264.958 ms and MVCC compaction in 1.387 ms, but it did not
measure archive size, storage growth, or commit-log reclamation. SMR0 adds a
benchmark-only high-churn workload with one maintained secondary index. It
changes no library or runtime behavior.

Command:

```text
cargo bench -p nimbus-storage --bench metadata-retention-baseline
```

Result: exit 0.

```text
Nimbus storage metadata-retention baseline
document_commits=2048 latest_sequence=2049 documents=256 configured_window_sequences=512
write_elapsed_ms=11781 writes_per_second=173.84
database_bytes=8957952 bytes_per_commit=4374.00
journal_records=2049 archive_bytes=3222918 archive_bytes_per_commit=1573.69 export_elapsed_ms=108
document_versions_before=2048 document_versions_after=768 document_versions_pruned=1280
index_versions_before=2048 index_versions_after=768 index_versions_pruned=1280
compaction_elapsed_ms=15 journal_records_pruned=0 journal_floor=0
```

This fixture is deliberately update-heavy and therefore a conservative redb
growth sample. Linear projections are capacity guides, not performance claims:

| Window | Journal-only archive projection | Gross redb projection from this fixture | Export projection from this fixture |
| ---: | ---: | ---: | ---: |
| 10,000 sequences | about 15.7 MB | about 43.7 MB | about 0.53 s |
| 50,000 sequences | about 78.7 MB | about 218.7 MB | about 2.64 s |
| 100,000 sequences | about 157.4 MB | about 437.4 MB | about 5.27 s |

The final checkpoint design changes PITR and storage constants, so SMR5 must
remeasure these values. The baseline is sufficient to reject an accidental
retain-all default and to avoid a multi-million-sequence hidden preset.

## Bounded Shipped Profile Decision

SMR0 ratifies the first shipped profile:

| Resource | Window |
| --- | ---: |
| Document versions | 100,000 sequences |
| Index versions | 100,000 sequences |
| CDC cursor validity | 50,000 sequences |
| PITR target validity | 100,000 sequences |
| Maintenance eligibility step | 10,000 new applied sequences |

Rationale:

- Document and index windows are equal because Nimbus promises indexed
  historical reads inside the document-history window. Convex's much shorter
  index window does not transfer to Nimbus's versioned-index semantics.
- PITR keeps the conservative 100,000-sequence window. CDC can expire at
  50,000 while the shared physical journal remains available for the longer
  PITR dependency.
- A 10,000-sequence step bounds incremental checkpoint replay and avoids a
  maintenance write on every mutation. SMR3 can also use a time-based wakeup so
  low-write tenants eventually reclaim eligible history.
- `retain_all` remains an explicit operator override.
- These are sequence capacity bounds. They do not promise a number of hours or
  days because workload rates differ. Public/operator text must state that
  fact.
- SMR5 can revise the numbers before closeout only from recorded checkpoint,
  restore, latest-path, provider, and steady-state measurements. It cannot
  restore an accidental unbounded default.

## Verification

| Command | Result |
| --- | --- |
| `bash -n scripts/verify-storage-metadata-retention.sh` | Passed. |
| `bash scripts/verify-storage-metadata-retention.sh` | Expected red: `Summary: 7 passed, 11 failed`, exit 1. |
| `cargo check -p nimbus-storage --bench metadata-retention-baseline` | Passed. |
| `cargo bench -p nimbus-storage --bench metadata-retention-baseline` | Passed with the exact calibration above. |
| `cargo fmt --all --check` | Passed after formatting the benchmark. |

No production Rust module, provider schema, Engine lifecycle, public API, or
runtime default changed in SMR0.
