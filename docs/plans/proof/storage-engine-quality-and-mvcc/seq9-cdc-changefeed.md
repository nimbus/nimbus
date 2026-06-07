# SEQ9 CDC Changefeed

status: done

## Scope

SEQ9 adds a typed storage-layer CDC/changefeed surface backed by the durable
tenant event journal. Consumers bootstrap from an explicit materialized
snapshot cut, resume from a typed cursor, and page authoritative
`TenantEventRecord` payloads. The implementation does not create a second log:
changefeeds compose the existing durable journal bootstrap and stream APIs.

The first public surface is intentionally storage/service level. Adapter-facing
exposure, compatibility docs, and support-state matrices remain part of
SEQ12/SEQ14, where unsupported adapter extensions must fail closed rather than
claiming stock SDK parity.

## Read-Before-Edit Checklist

- `docs/plans/storage-engine-quality-and-mvcc-plan.md`
- `crates/nimbus-storage/src/changefeed.rs`
- `crates/nimbus-storage/src/store/journal_stream.rs`
- `crates/nimbus-storage/src/traits/mod.rs`
- `crates/nimbus-storage/src/lib.rs`
- `crates/nimbus-storage/src/changefeed/tests.rs`
- `crates/nimbus-storage/src/tests/sqlite_foundation/journal.rs`
- `crates/nimbus-engine/src/persistence/tenant.rs`
- `crates/nimbus-engine/src/persistence/tenant/journal.rs`
- `crates/nimbus-engine/src/service/queries/journal.rs`
- `crates/nimbus-engine/src/lib.rs`

## Implementation Evidence

| Area | Evidence |
| --- | --- |
| Typed handles and cursors | `ChangefeedHandle` records handle id, snapshot cut, and cursor floor. `ChangefeedCursor` carries the handle plus last consumed sequence and supports validated handle rotation. |
| Snapshot-to-log handoff | `ChangefeedBootstrap::from_durable_bootstrap(...)` converts `DurableJournalBootstrap` into an explicit snapshot, handle, resume cursor, latest sequence, and cursor floor. The cursor resumes after the snapshot's applied sequence, so snapshot rows and catch-up events do not overlap. |
| Event pages | `ChangefeedPage::from_durable_page(...)` converts durable journal records into `ChangefeedEvent` values after validating record integrity. Each event carries sequence, timestamp, authoritative `TenantEventKind` payloads, and the original `TenantEventRecord`. |
| Retention-expired errors | CDC maps durable journal floor misses into `HistoricalReadErrorKind::RetentionExpired`. Cursor handle rotation also rejects cursors below the new handle floor before streaming. |
| All-provider storage surface | redb, SQLite, Postgres, MySQL, and libSQL tenant stores expose `export_changefeed_bootstrap(...)` and `stream_changefeed(...)` via the shared storage implementation. The `DurableJournal` trait now has default CDC methods for every backend that implements the durable journal capability. MySQL durable-journal streams and bootstrap snapshots derive `cursor_floor` from the retained `commit_log` minimum sequence, matching the retained-cursor behavior already used by redb, SQLite, and Postgres. |
| Engine service surface | `Service` exposes sync and async `export_changefeed_bootstrap(...)` and `stream_changefeed(...)` methods through the existing tenant operation guard and persistence executor path. |

## Verification Evidence

| Command | Result |
| --- | --- |
| `cargo test -p nimbus-storage changefeed -- --nocapture` | Passed: `2 passed, 0 failed`, `295 filtered out`. Covers redb snapshot-to-log handoff, no duplicate pages across cursor resume, handle rotation, table lifecycle, schema, index lifecycle, document write, trigger delivery payloads, and SQLite retention-expired mapping. |
| `cargo test -p nimbus-storage durable_journal_stream -- --nocapture` | Passed: `2 passed, 0 failed`, `303 filtered out`. Covers retained cursor-floor rejection for redb and SQLite, and the verifier now anchors the equivalent MySQL `cursor_floor` implementation in production code. |
| `cargo check -p nimbus-engine` | Passed. Confirms storage, all provider stores, persistence delegates, and engine service CDC APIs compile in production code. |

## External Fixture State

SEQ9 landed before the final Docker-backed provider closeout. The CDC surface
is implemented through each backend's existing durable journal capability and
compiles for Postgres/MySQL/libSQL. Later SEQ3/SEQ4 closeout runs supplied the
live MySQL/libSQL document/index evidence required before SEQ14.

## SEQ9 Closeout

SEQ9 is complete for the storage/service CDC control plane. Nimbus now has typed
changefeed handles and cursors, explicit snapshot cuts, no-miss/no-duplicate
journal handoff tests, handle rotation validation, typed retention-expired
errors, authoritative tenant-event pages, all-provider storage APIs, and engine
service wrappers over the existing durable journal path.
