# MIK0 Baseline

Date: 2026-09-09. Baseline `main @ 63fde8a81`. Worktree
`~/src/github.com/nimbus/nimbus-worktrees/mysql-index-keyspace`, branch
`codex/mysql-index-keyspace`.

## Trigger

PR #334 (`codex/nimbus-ui-rebuild-phase4`) failed the hosted MySQL job twice
on `projection_mysql_two_engine_takeover_rejects_late_old_document_schema_and_delete`
(`crates/nimbus-system/src/projection/reconciliation_tests.rs`). The Phase 3
base passed the same job. The Phase 4 system schema declares 69 indexes
(62 on main). Each maintained index is one InnoDB key on the shared
per-tenant `documents` table; with the primary key that is 70 keys against
the InnoDB limit of 64.

- Hosted job log: `mik0-pr334-hosted-job.txt`.
- Local repro with a temporary `eprintln` at the projection warn site:
  `mik0-pr334-local-repro.txt`. It shows MySQL error 1069 followed by
  "committer lease durable sequence 19 exceeds recovered storage head 18"
  (`StorageErrorKind::Corruption`), because the DDL statements before the
  failing one committed implicitly (finding F2).

## Fail-before test

`mysql_schema_with_more_than_sixty_four_indexes_applies_and_reads` in
`crates/nimbus-storage/src/tests/mysql_provider/schema.rs`: one table, 70
single-field number indexes, two documents inserted before the schema, then
an index scan on the first and the last index.

Command (fixture `bash scripts/external-provider-fixture.sh up mysql`,
container `nimbus-external-provider-tests-mysql-1`, image mysql:8.4):

```
NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES=1 \
NIMBUS_MYSQL_URL="mysql://root:fixture-mysql-root@127.0.0.1:3306/test" \
cargo test -p nimbus-storage --features mysql mysql_schema_with_more_than_sixty_four_indexes
```

Result on the baseline (`mik0-fail-before.txt`):

```
panicked at crates/nimbus-storage/src/tests/mysql_provider/schema.rs:294:14:
a schema with more than 64 indexes should apply on MySQL: Storage { kind: Other,
message: "Server error: `ERROR 42000 (1069): Too many keys specified; max 64 keys allowed'" }
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 462 filtered out
```

No production behavior changed in this task.
