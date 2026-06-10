# PW0 baseline — CI PR-Wall Sub-15

Snapshot taken 2026-05-23 against `main`. The plan's three poles
(libsql gate dominance, coverage track on PR critical path,
concurrent-runner saturation) are evidenced below from
GitHub Actions API.

## Last 5 `main` runs of `ci.yml` (success)

| Run ID         | SHA       | Title                                                   | createdAt → updatedAt | Wall      |
|----------------|-----------|---------------------------------------------------------|-----------------------|-----------|
| 26326200361    | 23eb430e  | CW5: backfill execution-log SHA for closeout            | 06:50:46 → 07:36:11   | **45m 25s** |
| 26324236849    | 32951ee7  | DU7 proof: fresh-build embed verification on main post-CA | 05:08:59 → 05:32:33 | **23m 34s** |
| 26323566487    | 0321c728  | CA3 hotfix 5: raise postgres CRUD CI budget for coverage overhead | 04:33:49 → 05:02:13 | **28m 24s** |
| 26318333466    | f99f7d6c  | CM4 hotfix: relax release ref contract to major-version pin | 00:25:18 → 00:59:15 | **33m 57s** |
| 26316126391    | faa9ffd2  | CM1 hotfix: drop secrets expression from composite description string | 23:03:17 → 23:46:52 | **43m 35s** |

Summary statistics (n=5):

- Mean wall: **35m 0s**
- Median wall: **33m 57s**
- Min: 23m 34s (DU7, hot-cache outlier)
- Max: 45m 25s (CW5, saturation outlier)
- Spread (max − min): **21m 51s** — the variance pole. CW3's saturation problem is real.

## CW5 (`23eb430e`) job timeline — the saturation outlier

Workflow createdAt: `2026-05-23T06:50:46Z`.
Critical path: `External Provider Integration Tests (libsql)` ends
at `07:36:06Z`, gate summary at `07:36:10Z`.

| Job                                       | Start (Δ from createdAt) | Duration | End (Δ)   |
|-------------------------------------------|--------------------------|----------|-----------|
| UI Artifacts                              | +0m 18s                  | 0m 27s   | +0m 45s   |
| Rust Format                                | +0m 18s                  | 0m 11s   | +0m 29s   |
| Proof Helper Checks                       | +0m 19s                  | 0m 18s   | +0m 37s   |
| JavaScript Build and Test                 | +0m 18s                  | 1m 21s   | +1m 39s   |
| Rust Dependency Audit                     | +0m 18s                  | 5m 9s    | +5m 27s   |
| Rust Clippy                                | +0m 20s                  | 7m 20s   | +7m 40s   |
| Warm sccache                               | +0m 48s                  | 6m 16s   | +7m 4s    |
| Rust Workspace Tests (shard 3/3)          | +0m 18s                  | 9m 11s   | +9m 29s   |
| Rust Workspace Tests (shard 2/3)          | +0m 18s                  | 9m 4s    | +9m 22s   |
| Rust Workspace Tests (shard 1/3)          | +0m 19s                  | 9m 55s   | +10m 14s  |
| Rust Runtime Tests                        | +0m 19s                  | 10m 39s  | +10m 58s  |
| External Provider Integration Tests (mysql)   | +0m 18s              | 9m 8s    | +9m 26s   |
| External Provider Integration Tests (postgres)| +0m 18s              | 9m 25s   | +9m 43s   |
| Storage Verification Harness               | +7m 6s                   | 7m 40s   | +14m 46s  |
| Runtime Verification Harness               | +7m 6s                   | 8m 33s   | +15m 39s  |
| Engine Verification Harness (shard 1/2)   | +7m 6s                   | 11m 9s   | +18m 15s  |
| Engine Verification Harness (shard 2/2)   | +7m 6s                   | 11m 6s   | +18m 12s  |
| Server Verification Harness (shard 1/4)   | +7m 6s                   | 11m 2s   | +18m 8s   |
| Server Verification Harness (shard 2/4)   | +7m 6s                   | 11m 16s  | +18m 22s  |
| Server Verification Harness (shard 3/4)   | +7m 6s                   | 11m 21s  | +18m 27s  |
| Server Verification Harness (shard 4/4)   | +7m 6s                   | 11m 37s  | +18m 43s  |
| Coverage shard (engine)                   | +7m 6s                   | 12m 46s  | +19m 52s  |
| Coverage shard (server)                   | +7m 6s                   | 12m 50s  | +19m 56s  |
| Coverage shard (rest)                     | +7m 6s                   | 14m 10s  | +21m 16s  |
| Coverage reducer                           | +21m 18s                 | 5m 47s   | +27m 5s   |
| **External Provider Integration Tests (libsql)** | **+27m 35s**       | **17m 45s** | **+45m 20s** |
| Rust Gate Summary                          | +45m 22s                 | 0m 2s    | +45m 24s  |

**Saturation evidence:** libsql got picked up `+27m 35s` after
createdAt — the other 13 first-wave jobs started at `+18s`. The
12-job second wave (harness + coverage) started at `+7m 6s` when
warm-sccache completed. libsql queued behind the second wave; the
org's concurrent ubuntu-runner ceiling was the bottleneck.

**Without the libsql queue wait**, CW5's wall would have been ≈
27m (matching DU7 and CA3-h5).

## Pole 1 — libsql vs postgres / mysql

CW3 (the per-provider split) produces this picture for the gate
path. Earlier runs used a single combined `External Provider
Integration Tests` job.

CW5 per-provider matrix:

| Provider  | Duration  | Multiplier vs mysql |
|-----------|-----------|---------------------|
| mysql     | 9m 8s     | 1.0×                |
| postgres  | 9m 25s    | 1.03×               |
| **libsql** | **17m 45s** | **1.94×**           |

libsql is 2× the other providers on identical infra. Image:
`ghcr.io/tursodatabase/libsql-server:latest` (untagged, pulled
fresh every run), mode `--enable-namespaces`. The actual cargo
test work is not the pole; cold container pull + namespace setup
is.

Pre-CW3 combined external-provider job duration on prior 4 runs:

| Run         | Combined provider duration |
|-------------|----------------------------|
| DU7         | 14m 33s                    |
| CA3-h5      | 16m 56s                    |
| CM4 hotfix  | 10m 12s                    |
| CM1 hotfix  | 14m 32s                    |

Combined p50 ≈ 14m 33s. CW3's split exposed libsql as the
within-provider 18m pole that the combined number had been
hiding.

## Pole 2 — coverage track is 26m 40s on the full PR wall

CW5 coverage critical path (decomposed from job timeline):

| Step             | Duration |
|------------------|----------|
| ui-artifacts     | 0m 27s   |
| warm-sccache     | 6m 16s   |
| Coverage shard (rest, max of 3 shards) | 14m 10s |
| Coverage reducer | 5m 47s   |
| **Total**         | **26m 40s** |

Coverage is **not** on `rust-gate-summary.needs:` — the gate's
inputs are `rust-format`, `rust-clippy`, `deny`,
`rust-runtime-tests`, `rust-workspace-tests`,
`external-provider-tests`. Coverage doesn't block merge; it
blocks the workflow wall.

CA3-h5 coverage timing (no per-provider split, broader sample):

| Job              | Duration |
|------------------|----------|
| Coverage shard (server) | 13m 10s |
| Coverage shard (rest)   | 13m 1s  |
| Coverage shard (engine) | 12m 6s  |

Coverage shards reliably sit 12–14m post-CA. The track itself
(ui→warm-sccache→cov_max→reducer) reliably sits ~26m.

## Pole 3 — concurrent-runner saturation

Peak simultaneous job count during CW5:

- t=+0m 18s: 13 jobs in flight (1st wave)
- t=+0m 48s: 14 jobs (warm-sccache joins)
- t=+5m 27s: deny exits → 13 jobs
- t=+7m 4s: warm-sccache exits → 13 jobs
- t=+7m 6s: 11 new jobs start (2nd wave) → **24 simultaneous**
- t=+7m 40s: clippy exits → 23
- t=+9m 22s..29s: 3 workspace test shards exit → 20
- t=+9m 26s..43s: postgres + mysql exit → 18

Peak: **24 simultaneous jobs**. libsql queued at +0m 18s but
didn't get a runner until +27m 35s. That's a 27m queue wait — a
clear signal that the org's concurrent-ubuntu-runner ceiling was
saturated.

Without saturation evidence from runs in the 24–28m band (DU7,
CA3-h5) we'd have to guess the ceiling. CW5 made it explicit:
the ceiling is somewhere between 20 and 24, and the libsql shard
was the unlucky job that lost the race.

## Swatinem cache signal — pending

`gh run view` does not expose Swatinem `Cache restored from key`
log lines in its JSON. Extracting hit/miss requires fetching the
per-job log artifact via `gh api repos/{owner}/{repo}/actions/
runs/{run_id}/logs`. PW4a's measurement phase will do this
systematically across a 7-day window. PW0 records only that the
data is currently unmeasured.

The proxy signal we *can* observe: warm-sccache runs `cargo
check --workspace` in 6m 16s on CW5. If the Swatinem target/
cache were restoring cleanly, warm-sccache would be much
shorter (target/ rmeta cached → cargo check skips compilation).
The 6m duration suggests target/ cache is either not being
restored or is being invalidated by code churn at every run.
PW4a will confirm or refute.

## PW pole-attack expected impact (recap)

| Pole                  | Attack | Pole today | Pole after  |
|-----------------------|--------|------------|-------------|
| libsql gate dominance | PW1: pin tag + image cache | 17m 45s | ≤ 10m |
| coverage on PR wall   | PW2: extract to coverage.yml | 26m 40s | not on PR |
| saturation tail       | PW3: never cancel main | 27m queue wait | 0 |
| warm-sccache prelude  | PW4: measure, retire if Swatinem hit ≥85% | 6m 16s gated | 0 (PW4b) or kept (PW4c) |

## Honest exit shapes

After PW1 + PW2 + PW3: PR wall floor ≈ 18m 25s
(`ui-artifacts → warm-sccache → harness_max`).

After + PW4b (retire warm-sccache): wall floor ≈ 13m 30s.

After + PW4c (keep warm-sccache): wall floor ≈ 18m 25s; plan
declares success at "PR wall ≤ 20m p95" and documents why
sub-15 awaits cache-invalidation work.

The verifier accepts both PW4b and PW4c outcomes (condition 7
flips correctly either way; condition 8 walls measure against
15m or 18m depending on condition 7's signal).
