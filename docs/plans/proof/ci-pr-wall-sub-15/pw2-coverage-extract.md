# PW2 — coverage extracted to its own workflow

## Move

`coverage:` (matrix shard server/engine/rest) and `coverage-reduce:` —
together with the leader jobs they consumed (`ui-artifacts`,
`warm-sccache`) — are duplicated into `.github/workflows/coverage.yml`
and removed from `.github/workflows/ci.yml`.

`ci.yml` retains its own `ui-artifacts:` and `warm-sccache:` jobs
because the harness shards still depend on them. The coverage workflow
gets fresh copies of those leader jobs so it can run independently of
`ci.yml` on `main`-only schedules.

## Triggers

`coverage.yml`:

```yaml
on:
  push:
    branches: [main]
  schedule:
    - cron: "0 8 * * 1"   # weekly Monday 08:00 UTC
  workflow_dispatch:
```

No `pull_request:` trigger. Coverage will not run on PRs.

`ci.yml` retains `pull_request:`, `push:` (main), `workflow_dispatch:`,
and the weekly `schedule:` for the deny audit window.

## Concurrency on coverage.yml

```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}
```

A cancelled `main` coverage run would abandon the `lcov.info` upload
and create per-commit gaps in Codecov history. The branch-conditional
expression keeps the cancellation behaviour useful (rapid retriggers
on workflow_dispatch from non-main refs still cancel each other) while
protecting `main` from itself.

## Why this works

Coverage is **not** on `rust-gate-summary.needs:` —

```yaml
rust-gate-summary:
  needs: [rust-format, rust-clippy, deny, rust-runtime-tests,
          rust-workspace-tests, external-provider-tests]
```

— so removing it from the PR critical path does not weaken any
merge-blocking signal. The codecov upload moves from "every PR push"
to "every push to main + weekly". This is the same pattern the deny
audit already uses.

## Expected impact

CW5 coverage track on the PR critical path:

| Step             | Duration |
|------------------|----------|
| ui-artifacts     | 0m 27s   |
| warm-sccache     | 6m 16s   |
| Coverage shard (rest, max of 3) | 14m 10s |
| Coverage reducer | 5m 47s   |
| **Total**         | **26m 40s** |

After PW2, none of this runs on PRs. PR critical path becomes the
gate-only path (rust-format / rust-clippy / deny / rust-runtime-tests
/ rust-workspace-tests / external-provider-tests → rust-gate-summary).

Coverage runs that move to `main` no longer race PR jobs for
concurrent-runner slots, which also helps PW3's saturation attack
indirectly.

## Verifier

Verifier condition 5 passes after PW2:

```
[5] Coverage extracted: coverage.yml exists, ci.yml has no Coverage jobs
  PASS  coverage.yml ships with schedule + push.main; ci.yml has no Coverage jobs
```

Condition 4 (libsql pin) continues to pass — the PW1 pin moved with
the coverage job into `coverage.yml`, complete with the cache lane.
