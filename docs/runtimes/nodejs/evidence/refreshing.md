# Refreshing Node.js Runtime Evidence

This page documents the canonical maintainer workflow for refreshing Node.js
runtime compatibility evidence. The workflow is intentionally evidence-first:
lane metadata, fixture sync reports, status summaries, dashboards, trend
snapshots, architecture evidence, and public runtime evidence are produced by
one orchestration path.

## Supported Lanes

Nimbus currently carries these selectable Node.js compatibility lanes:

| Lane | Public role | Notes |
| --- | --- | --- |
| `node20` | Supported | Supported LTS compatibility lane. |
| `node22` | Default | Current default Node.js runtime target. |
| `node24` | Supported | Supported current-line compatibility lane. |

Future lanes should use the same `nodeNN` shape and start with checked-in lane
metadata under `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/lanes/`.

## One Command Path

Use `make node-compat-refresh` as the canonical entrypoint.

Dry-run the current lane without editing metadata:

```bash
make node-compat-refresh LANE=node22 DRY_RUN=1
```

Compare a lane against an upstream Node tag without applying fixture changes:

```bash
make node-compat-refresh LANE=node24 TAG=v24.15.0 COMPARE_UPSTREAM=1
```

Apply a deliberate lane tag and fixture refresh:

```bash
make node-compat-refresh LANE=node24 TAG=v24.15.0 APPLY=1
```

Run representative live Node test checks during the refresh when validating runtime
behavior, not just metadata and generated reports:

```bash
make node-compat-refresh LANE=node22 DRY_RUN=1 RUN_SLICES=1
```

`FORCE=1` is reserved for deliberate fixture apply work over dirty local fixture
paths. Prefer a clean tree or a tightly scoped diff review before using it.

## What The Refresh Runs

The refresh command performs these steps:

| Step | Purpose |
| --- | --- |
| `sync` | Builds or applies the fixture sync plan for the selected lane. |
| `report:*` | Optional representative live Node test checks when `RUN_SLICES=1`. |
| `expectations` | Validates Rust watchpoint expectations against the current harness inventory. |
| `status` | Recomputes suite-wide lane denominators and classifications. |
| `inventory` | Recomputes the selected lane's vendored fixture inventory. |
| `dashboard` | Aggregates status, inventory, slice, canary, and oracle evidence. |
| `trends` | Compares current evidence against the checked-in latest baseline. |
| `publish` | Publishes engineering evidence under `docs/architecture/runtime/node-compat-evidence/latest/`. |
| `publish_docs` | Publishes developer-facing pages under `docs/runtimes/nodejs/evidence/`. |
| `claims` | Validates public claim mappings against the canary registry. |

The refresh report is written to `target/node-compat/refresh/<lane>-refresh.md`
and `.json`.

## Review Checklist

Before committing a refresh, review:

- `target/node-compat/refresh/<lane>-refresh.md`
- `docs/architecture/runtime/node-compat-evidence/latest/status-summary.md`
- `docs/architecture/runtime/node-compat-evidence/latest/dashboard-summary.md`
- `docs/architecture/runtime/node-compat-evidence/latest/trend-summary.md`
- `docs/runtimes/nodejs/evidence/latest.md`
- the lane-specific page under `docs/runtimes/nodejs/evidence/<lane>.md`

Do not treat expected failures, known gaps, skipped fixtures, or unclassified
fixtures as support claims. Only `Passed` fixtures, passing canaries, and
explicit classifications may support public compatibility language.
