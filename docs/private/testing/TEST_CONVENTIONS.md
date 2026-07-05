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
