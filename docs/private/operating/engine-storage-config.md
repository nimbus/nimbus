# Engine and Storage Configuration

This runbook lists every environment variable that `nimbus-engine` and
`nimbus-storage` read. Use it to tune a node or to add a new setting. The two
crates read 71 variables. 54 of them change behavior. 17 of them are test
harness inputs.

## Parse contract

- `EngineConfig::from_env()` in `crates/nimbus-engine/src/config.rs` parses
  the engine variables. It also parses the storage variables through
  `StorageConfig::from_lookup`.
- `StorageConfig::from_env()` in `crates/nimbus-storage/src/config.rs` parses
  only the storage variables.
- The composition root calls `from_env()` one time. The CLI start path and the
  CLI `backup` and `object-storage` commands attach the result with
  `EnginePersistenceConfig::with_engine_config`.
- Other modules get their values from the struct. They do not read the
  environment.
- `Engine::new` and the other direct constructors use `EngineConfig::default()`.
  The default is the shipped behavior.
- Tests set fields on the struct. They do not set environment variables.
- An absent, malformed, or zero integer keeps its default. The exceptions are
  the backoff and timeout values, which accept zero.
- A flag of type `presence` is on when the variable exists, whatever its value.

To add a setting, add a field and its default to the owning struct. Then add
one row to the table below.

## Inventory

The mutation caps build 14 variable names with `format!` in
`engine::mutations::caps`. The table lists each name in full.

| Variable | Type | Default | Owning module |
| --- | --- | --- | --- |
| `NIMBUS_MUTATION_JOURNAL_BATCH_MAX` | positive integer | 256 | `nimbus-engine` `config` |
| `NIMBUS_MUTATION_JOURNAL_COALESCE_MICROS` | integer, microseconds | 0 | `nimbus-engine` `config` |
| `NIMBUS_COMMITTER_PUBLISHER_BATCH_MAX` | positive integer | 256 | `nimbus-engine` `config` |
| `NIMBUS_COMMITTER_PUBLISHER_COALESCE_MICROS` | integer, microseconds | 750 | `nimbus-engine` `config` |
| `NIMBUS_MUTATION_OCC_MAX_RETRIES` | positive integer | 4 | `nimbus-engine` `config` |
| `NIMBUS_MUTATION_OCC_INITIAL_BACKOFF_MS` | integer, milliseconds | 100 | `nimbus-engine` `config` |
| `NIMBUS_MUTATION_OCC_MAX_BACKOFF_MS` | integer, milliseconds | 2000 | `nimbus-engine` `config` |
| `NIMBUS_COMMITTER_PUBLISHER_RETRY_LIMIT` | positive integer | 4 | `nimbus-engine` `config` |
| `NIMBUS_COMMITTER_PUBLISHER_RETRY_INITIAL_MS` | integer, milliseconds | 1 | `nimbus-engine` `config` |
| `NIMBUS_COMMITTER_PUBLISHER_RETRY_MAX_MS` | integer, milliseconds | 100 | `nimbus-engine` `config` |
| `NIMBUS_COMMITTER_INBOX_SIZE` | positive integer | 128 | `nimbus-engine` `config` |
| `NIMBUS_COMMITTER_SEND_TIMEOUT_MS` | integer, milliseconds | 500 | `nimbus-engine` `config` |
| `NIMBUS_COMMITTER_PUBLISHER_QUEUE_SIZE` | positive integer | 32 | `nimbus-engine` `config` |
| `NIMBUS_COMMITTER_PUBLISHER_SEND_TIMEOUT_MS` | integer, milliseconds | 500 | `nimbus-engine` `config` |
| `NIMBUS_COMMITTED_OBSERVER_QUEUE_CAPACITY` | positive integer | 4096 | `nimbus-engine` `config` |
| `NIMBUS_COMMITTED_OBSERVER_QUEUE_HIGH_WATERMARK` | positive integer | 3072 | `nimbus-engine` `config` |
| `NIMBUS_TENANT_MUTATION_ISOLATE_CEILING` | positive integer | 16 | `nimbus-engine` `config` |
| `NIMBUS_PREPARE_CONCURRENCY` | positive integer | host parallelism, at most 4 | `nimbus-engine` `config` |
| `NIMBUS_SHADOW_CONFLICT_SAMPLE_EVERY` | positive integer | 16 | `nimbus-engine` `config` |
| `NIMBUS_SHADOW_CONFLICT_WINDOW_MAX` | positive integer | 64 | `nimbus-engine` `config` |
| `NIMBUS_OVERLOAD_ERROR_REPORT_EVERY` | positive integer | 100 | `nimbus-engine` `config` |
| `NIMBUS_SHADOW_CAP_REPORT_EVERY` | positive integer | 100 | `nimbus-engine` `config` |
| `NIMBUS_WIDE_READ_SET_WARN_THRESHOLD` | positive integer | 1000 | `nimbus-engine` `config` |
| `NIMBUS_COMMIT_TRACE_THRESHOLD_MS` | integer, milliseconds | off; 500 when set but malformed | `nimbus-engine` `config` |
| `NIMBUS_QUERY_PROFILE` | presence | off | `nimbus-engine` `config` |
| `NIMBUS_TENANT_LOAD_PROFILE` | presence | off | `nimbus-engine` `config` |
| `NIMBUS_WRITE_LOG_MIN_RETENTION_SECS` | positive integer | 30 | `nimbus-engine` `engine::mutations::write_log` |
| `NIMBUS_WRITE_LOG_MAX_RETENTION_SECS` | positive integer | 300, at least the minimum | `nimbus-engine` `engine::mutations::write_log` |
| `NIMBUS_WRITE_LOG_SOFT_MAX_BYTES` | positive integer | 33554432 | `nimbus-engine` `engine::mutations::write_log` |
| `NIMBUS_PROPOSED_MUTATION_READ_BYTES` | positive integer | 16777216 | `nimbus-engine` `engine::mutations::caps` |
| `NIMBUS_MUTATION_READ_BYTES` | positive integer | not enforced | `nimbus-engine` `engine::mutations::caps` |
| `NIMBUS_PROPOSED_MUTATION_WRITE_BYTES` | positive integer | 16777216 | `nimbus-engine` `engine::mutations::caps` |
| `NIMBUS_MUTATION_WRITE_BYTES` | positive integer | not enforced | `nimbus-engine` `engine::mutations::caps` |
| `NIMBUS_PROPOSED_MUTATION_DOCUMENTS_SCANNED` | positive integer | 32000 | `nimbus-engine` `engine::mutations::caps` |
| `NIMBUS_MUTATION_DOCUMENTS_SCANNED` | positive integer | not enforced | `nimbus-engine` `engine::mutations::caps` |
| `NIMBUS_PROPOSED_MUTATION_DOCUMENTS_WRITTEN` | positive integer | 16000 | `nimbus-engine` `engine::mutations::caps` |
| `NIMBUS_MUTATION_DOCUMENTS_WRITTEN` | positive integer | not enforced | `nimbus-engine` `engine::mutations::caps` |
| `NIMBUS_PROPOSED_MUTATION_INDEX_RANGE_CALLS` | positive integer | 4096 | `nimbus-engine` `engine::mutations::caps` |
| `NIMBUS_MUTATION_INDEX_RANGE_CALLS` | positive integer | not enforced | `nimbus-engine` `engine::mutations::caps` |
| `NIMBUS_PROPOSED_SYSTEM_MUTATION_WRITE_BYTES` | positive integer | 134217728 | `nimbus-engine` `engine::mutations::caps` |
| `NIMBUS_SYSTEM_MUTATION_WRITE_BYTES` | positive integer | not enforced | `nimbus-engine` `engine::mutations::caps` |
| `NIMBUS_PROPOSED_SYSTEM_MUTATION_DOCUMENTS_WRITTEN` | positive integer | 40000 | `nimbus-engine` `engine::mutations::caps` |
| `NIMBUS_SYSTEM_MUTATION_DOCUMENTS_WRITTEN` | positive integer | not enforced | `nimbus-engine` `engine::mutations::caps` |
| `NIMBUS_PROPOSED_TENANT_WRITE_BYTES_PER_SEC` | positive integer | 1048576 | `nimbus-engine` `tenant::write_rate` |
| `NIMBUS_TENANT_WRITE_BYTES_PER_SEC` | positive integer | not enforced | `nimbus-engine` `tenant::write_rate` |
| `NIMBUS_TENANT_WRITE_RATE_WINDOW_MS` | positive integer, milliseconds | 1000 | `nimbus-engine` `tenant::write_rate` |
| `NIMBUS_TENANT_WRITE_RATE_REPORT_EVERY` | positive integer | 100 | `nimbus-engine` `tenant::write_rate` |
| `NIMBUS_SQLITE_MAX_READ_CONNECTIONS` | positive integer | host parallelism, at least the SQLite minimum | `nimbus-storage` `config` |
| `NIMBUS_CONTROL_PLANE_PROFILE` | presence | off | `nimbus-storage` `config` |
| `NIMBUS_REDB_OPEN_PROFILE` | presence | off | `nimbus-storage` `config` |
| `NIMBUS_REDB_IO_PROFILE` | presence | off | `nimbus-storage` `config` |
| `NIMBUS_REDB_JOURNAL_PROFILE` | presence | off | `nimbus-storage` `config` |
| `NIMBUS_SQLITE_OPEN_PROFILE` | presence | off | `nimbus-storage` `config` |
| `NIMBUS_PROFILE_ONLY_COLD_SAMPLES` | presence | off | `nimbus-storage` `config` |

A `PROPOSED` cap is a shadow limit. The engine logs a sampled warning when a
mutation goes above it. The matching cap without `PROPOSED` rejects the
mutation. `NIMBUS_PROFILE_ONLY_COLD_SAMPLES` limits the storage profiles and
the tenant-load profile to cold samples.

## Test harness inputs

These variables select fixtures, cases, shards, and seeds. They do not change
engine or storage behavior. Two typed structs parse them. They are separate
from the production configuration structs.

- `StorageTestHarness` is in `crates/nimbus-storage/src/config.rs`. It is
  not gated to test builds. The generated-history simulation harness in the
  library reads the case filter and the shard selector. Its
  `external_providers` field holds the provider fixture inputs. Engine
  provider tests use this field too.
- `EngineTestHarness` in `crates/nimbus-engine/src/config.rs` exists only in
  test builds.

Tests read `CARGO_MANIFEST_DIR` at run time, not with `env!`. Taxonomy rule
F2 requires this, because nextest remaps the directory for an archived run.

| Variable | Purpose |
| --- | --- |
| `CARGO_MANIFEST_DIR` | Crate root for source-tree scans and artifacts |
| `NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES` | Fails a provider test when its fixture is absent |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES` | Skips implicit provider fixtures |
| `NIMBUS_TEST_POSTGRES_URL` | PostgreSQL fixture URL |
| `NIMBUS_MYSQL_URL` | MySQL fixture URL |
| `NIMBUS_LIBSQL_URL`, `NIMBUS_LIBSQL_AUTH_TOKEN` | libSQL primary fixture |
| `NIMBUS_LIBSQL_ADMIN_URL`, `NIMBUS_LIBSQL_ADMIN_AUTH_HEADER` | libSQL admin fixture |
| `NIMBUS_VERIFY_CASE` | Verification case filter |
| `NIMBUS_HARNESS_SHARD` | Verification shard, as `N/M` |
| `NIMBUS_STORAGE_CONFORMANCE_SEED` | Generated-history seed |
| `NIMBUS_SIC6_CRASH_DB` | SQLite crash-durability child database path |
| `NIMBUS_PPSC_REPLAY_SCENARIO_JSON`, `NIMBUS_PPSC_BACKEND` | PPSC scenario replay |
| `NIMBUS_ELLE_CLI_JAR`, `NIMBUS_ELLE_JAVA_BIN` | Elle checker lane |

## Readers outside these crates

Two other crates read variables from this table. They keep their own parse.

- `nimbus-bridge` `mutation_retry.rs` reads the three
  `NIMBUS_MUTATION_OCC_*` variables for the bridge retry loop.
- `nimbus-crypto` `runtime.rs` reads `NIMBUS_PROFILE_ONLY_COLD_SAMPLES`.
