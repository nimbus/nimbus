# Nimbus Test Conventions

This is the canonical test story for the test-infra rearchitecture. The binding
quality bar remains `docs/private/plans/test-infra-rearchitecture/TEST_STANDARD.md`.

## Naming

Use `<surface>::<case_class>::<behavior>_<condition>_<expectation>` as the
placement and name shape.

- `surface`: the user-visible or architecture-owned surface under test, such as
  storage, mutation_path, runtime_bridge, convex_adapter, or tenant_isolation.
- `case_class`: one of `smoke`, `main`, `edge`, `error`, `recovery`, or
  `real_world`.
- Test function names state behavior, condition, and expected outcome. Avoid
  names that only restate an API call.

## Layout

Put case classes in module paths so coverage is visible from the tree:

```text
src/tests/<surface>/<case_class>.rs
tests/<surface>/<case_class>.rs
```

Keep shared fixtures beside the surface they serve unless they are reused across
multiple crates. Widely shared deterministic fixtures belong in `nimbus-testing`.

## Purity And Extent

Classify tests by purity and extent.

- Pure tests: no IO, deterministic, single-threaded. Prefer these for large
  matrices and edge/error coverage.
- Local integration tests: filesystem, loopback listeners, subprocesses, or
  runtime isolates. Keep dependencies explicit and deterministic.
- External-provider tests: real Postgres, MySQL, libSQL, Node canaries, KVM, or
  OCI runtime stacks. These require a nextest group or ledger row.

PostgreSQL, MySQL, and libSQL storage/engine/system lanes use the owned fixture
interface in [external-provider-fixtures.md](./external-provider-fixtures.md).
Do not reproduce provider image, port, readiness, URL, or cleanup policy in a
workflow or local shell command.

Avoid wall-clock sleeps, hidden global state, and implicit network dependencies.
If a case is hard to test cleanly, improve the seam before expanding coverage.

## Ledger Workflow

`tests/taxonomy/exclusions.toml` is the only PR-tier exclusion ledger.
Every row needs `pattern`, `reason`, `evidence`, `measured_at`, `owner`, and
`issue`. Flaky quarantines also require `expiry`; expired quarantines fail the
taxonomy check.

After changing the ledger, regenerate the nextest filter section:

```bash
python3 scripts/test-taxonomy.py generate-nextest
```

Paste the generated block between the BEGIN/END markers in
`.config/nextest.toml`, then run:

```bash
python3 scripts/test-taxonomy.py check
```

## Local Profiles

Use the pinned cargo-nextest version from B1 onward: `0.9.138`.

```bash
cargo nextest list -P ci-pr --workspace --exclude nimbus-runtime --list-type binaries-only
cargo nextest list -P ci-nightly --workspace --exclude nimbus-runtime --list-type binaries-only
cargo nextest list -P ci-runtime -p nimbus-runtime --lib --list-type binaries-only
cargo nextest list -P ci-harness-required --workspace --run-ignored all --list-type binaries-only
cargo nextest list -P ci-harness-nightly --workspace --run-ignored all --list-type binaries-only
```

Harness profiles are explicitly scoped to `verification_harness_*` wrappers.
Do not use a bare ignored-test run as a harness lane.

## PPSC deterministic seed farm

The PPSC mutation-path farm has one local/CI interface:

```bash
make verify-ppsc-seed-farm
```

The default run selects 1,000 redb scenarios from seed 0. CI uses the same
target with four zero-based, non-overlapping shards:

```bash
make verify-ppsc-seed-farm \
  BACKEND=redb SEED_START=0 SEED_COUNT=1000 \
  SHARD_INDEX=0 SHARD_COUNT=4
```

Replay one failure without inheriting range inputs:

```bash
NIMBUS_PPSC_SEED=83 NIMBUS_PPSC_BACKEND=redb \
  make verify-ppsc-seed-farm
```

The shared contract rejects zero counts, empty shards, unknown backends, and
zero-test filters. It prints and writes `selected`, `executed`, `passed`,
`failed`, and `retained` counts. Before each scenario it atomically writes an
interruption bundle containing the exact generated scenario and replay command;
success removes that marker, while a caught assertion replaces it with a
failure bundle. The default artifact root is
`target/ppsc-seed-farm/shard-<index>-of-<count>/`. A green result requires a
complete `summary.json`; stale owned artifacts are cleared at invocation start
without removing foreign files from an explicitly supplied directory.

Confirmed failures graduate to the versioned retained seed list in
`crates/nimbus-testing/src/ppsc/scenario.rs`. The ordinary redb/SQLite and live
PostgreSQL/MySQL/libSQL differential tests replay that retained corpus through
their production Engine adapters. The 1,000-scenario claim belongs only to the
redb seed-farm jobs; live-provider, Hermitage, Loom, and crash-matrix results
remain separate evidence.

Deterministic simulation proves the enumerated histories against the PPSC
legal-state auditor and production Engine interface, including a settled redb
Engine restart/reopen. That lifecycle operation gracefully drains workers and
is not process-loss evidence; abrupt commit-phase loss is covered by the
separate PPSC crash matrix and fault seams. Neither surface proves unenumerated
schedules, provider transaction semantics, real network behavior, or external
serializability-checker results.
