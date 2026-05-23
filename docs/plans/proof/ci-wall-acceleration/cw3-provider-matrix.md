# CW3 — external provider tests per-provider matrix split

Splits `External Provider Integration Tests` (the 14.6m lateral pole on the
CW0 baseline) into a 3-shard matrix keyed on provider so postgres, mysql,
and libsql run in parallel instead of serialized inside one job.

## Why per-provider

The pre-CW3 job ran six cargo invocations sequentially:

```
cargo test -p nimbus-storage postgres_provider
cargo test -p nimbus-storage mysql_provider
cargo test -p nimbus-storage libsql_provider
cargo test -p nimbus-engine postgres_provider
cargo test -p nimbus-engine mysql_provider
cargo test -p nimbus-engine libsql_replica_provider
```

The within-provider tests use `serial_test::serial(<provider>)` to keep
schema setup/teardown deterministic against a single shared service. That
serialization is per-provider — across providers there is no shared state,
so they can fan out across runners cleanly. Splitting by provider preserves
the within-provider contract while letting the three providers progress in
parallel.

`cargo-nextest --partition` is *not* the right axis here: partitioning by
test hash would scatter the same provider's tests across multiple shards,
each of which would need to bring up that provider's service. Provider is
the natural seam.

## Shape

### Script contract

`scripts/test-external-providers.sh` reads `NIMBUS_PROVIDER_FILTER` and
dispatches to a single provider's invocations, or runs all three when the
filter is empty (preserving local-dev behavior):

```bash
case "${NIMBUS_PROVIDER_FILTER:-}" in
  postgres) run_postgres ;;
  mysql)    run_mysql ;;
  libsql)   run_libsql ;;
  "")       run_postgres; run_mysql; run_libsql ;;
  *)        echo "unknown NIMBUS_PROVIDER_FILTER=..." >&2; exit 1 ;;
esac
```

Each `run_<provider>` first asserts only that provider's URL env vars, then
runs both the `nimbus-storage` and `nimbus-engine` test selections for that
provider. A shard for `postgres` never depends on `NIMBUS_MYSQL_URL` being
set, so a stricter per-shard CI env would still work; the workflow keeps
all three URLs in the job-level `env:` for simplicity.

### Matrix shape

```yaml
strategy:
  fail-fast: false
  matrix:
    include:
      - provider: postgres
      - provider: mysql
      - provider: libsql
env:
  NIMBUS_PROVIDER_FILTER: ${{ matrix.provider }}
  NIMBUS_TEST_POSTGRES_URL: ...
  NIMBUS_MYSQL_URL: ...
  NIMBUS_LIBSQL_URL: ...
  NIMBUS_LIBSQL_ADMIN_URL: ...
```

`fail-fast: false` keeps a failure in one provider from cancelling the other
two — the gate-summary already aggregates all three into a single
`external-provider-tests` result for the Rust gate, so this is purely a
diagnostic-friendliness choice (you see all three failures at once on
divergent regressions).

### Service startup

The pre-CW3 job used GitHub Actions `services:` for postgres and mysql plus
a manual `docker run` for libsql. GH Actions `services:` cannot be
templated by matrix vars without pinning the image conditionally via expressions,
so CW3 retires the `services:` block entirely and starts every provider via
`docker run` gated on `if: matrix.provider == '<name>'`. The postgres and
mysql start steps now carry the same `--health-cmd / --health-interval /
--health-retries` semantics that `services:` previously provided, and the
wait loop polls the same `pg_isready` / `mysqladmin ping` probes.

```yaml
- name: Start postgres provider fixture
  if: matrix.provider == 'postgres'
  run: |
    docker run --detach --name nimbus-postgres-provider-tests \
      --publish 5432:5432 --env POSTGRES_DB=postgres ... postgres:16
```

The libsql startup is unchanged structurally — just gated on
`matrix.provider == 'libsql'` now.

### Naming

The job display name now interpolates the provider:

```
External Provider Integration Tests (postgres)
External Provider Integration Tests (mysql)
External Provider Integration Tests (libsql)
```

`needs['external-provider-tests'].result` in `rust-gate-summary` continues
to aggregate the matrix to a single result (GitHub Actions reports a matrix
job's `.result` as the worst of its shards), so the existing gate logic
needs no change.

## Local repro

```bash
# Run only the postgres shard locally
NIMBUS_PROVIDER_FILTER=postgres \
  NIMBUS_TEST_POSTGRES_URL='host=127.0.0.1 port=5432 ...' \
  make test-external-providers

# Run only the libsql shard locally
NIMBUS_PROVIDER_FILTER=libsql \
  NIMBUS_LIBSQL_URL=http://127.0.0.1:18080 \
  NIMBUS_LIBSQL_ADMIN_URL=http://127.0.0.1:18081 \
  make test-external-providers

# Run all three (unchanged from pre-CW3 default)
make test-external-providers
```

When `NIMBUS_PROVIDER_FILTER` is unset, behavior is byte-identical to the
pre-CW3 script: all six cargo invocations run sequentially in the same
order they always did.

## Expected wall delta

- **Pre-CW3 (CW0 baseline)**: `External Provider Integration Tests` ran 14.6m
  as a single un-sharded job.
- **CW3 with 3 provider shards**: each shard runs ~1/3 of the work, but
  service startup, cargo compile, and shared-key warm-up are paid in
  parallel rather than serial. Postgres is historically the slowest provider
  (bigger driver crate footprint, more comprehensive test surface), so the
  postgres shard is the expected wall pole at ~6-7m. The two faster shards
  (mysql, libsql) finish in roughly the same wall window but no longer queue
  behind postgres.

After CW3 the external-provider lateral pole drops to ~7m, which sits below
the warm-sccache + 1 server-harness-shard critical path established by CW1.

## Verifier evidence

```
[7] external-provider-tests job has provider matrix with ≥ 3 entries
  PASS  external-provider job has provider matrix axis with 3 entries
```

Condition 7 (`scripts/verify-ci-wall-acceleration.sh`) extracts the
`external-provider-tests:` job block and counts `- provider: <name>`
matrix entries; CW3 lands the include form so each entry matches the
regex `^[[:space:]]+-[[:space:]]+provider:[[:space:]]+[a-z]+`.

## Notes

- The matrix uses `include:` rather than a flat `provider:
  [postgres,mysql,libsql]` so the verifier's regex hits each entry as its
  own line. This matches the CW1 harness matrix shape.
- `serial_test::serial(<provider>)` keeps within-provider tests serialized
  within a shard. The pre-CW3 code already had per-provider serial groups,
  so nothing in the test crates changes.
- `make test-external-providers` is unchanged at the Makefile level — the
  per-provider filter is a script-internal contract, not a target axis.
- A future investigation could drop the postgres shard further by
  partitioning postgres tests via nextest if postgres alone becomes the
  wall pole post-CW3. Deferred until measured.
