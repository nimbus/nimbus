# CI PR-Wall Sub-15 Plan (PW)

Status: active
Owner: ci-pr-wall
Verifier: `scripts/verify-ci-pr-wall-sub-15.sh`
Target: PR-shaped `ci.yml` wall ≤ 15 minutes, p95 across the next 10
post-merge `main` runs.

## TL;DR

The CW plan landed (CW0..CW5, archived 2026-05-23). Post-CW `main`
runs sit at **23m–28m on good days** and have a **45m tail on
saturation days**. PW attacks the three remaining poles directly so
PR wall lands and stays under 15 minutes.

The three poles, ranked by leverage:

1. **`External Provider Integration Tests (libsql)` ≈ 18m** —
   gate-bound. Postgres and mysql complete the same shape in
   ~9m; libsql is 2× slower for identical infra. The image
   uses `ghcr.io/tursodatabase/libsql-server:latest` (untagged)
   and `--enable-namespaces`, both of which cost wall time on
   every PR.
2. **Coverage track ≈ 26m 40s** — full-wall pole. `ui-artifacts
   (27s) → warm-sccache (6m 16s) → coverage_rest (14m 10s) →
   coverage_reducer (5m 47s)`. Coverage is **not** on the merge
   gate; it costs PR wall without gating anything PR-bound.
3. **Concurrent-runner saturation** — variance pole. CW5
   bursted to ~24 simultaneous jobs at the warm-sccache fan-out
   and `libsql` queued 27 minutes before getting a runner. This
   is what turned a 27m run into a 45m run; it will keep
   happening until we cap concurrency or stagger the second
   wave.

PW1 attacks pole 1 (libsql). PW2 attacks pole 2 (coverage off
PR). PW3 attacks pole 3 (concurrency cap). PW4 is the only
stretch item — it asks whether `warm-sccache` is still net
positive now that Swatinem v2 caches `target/` with
`save-always: true` (CC9). PW1+PW2+PW3 alone should land wall
at ~17m. PW4 closes the last 2m if Swatinem cache data
supports retirement; otherwise PW4 documents *why* warm-sccache
stays and the plan declares success at the floor we can prove.

## Why this plan exists

Wall-time evidence from the last 3 `main` runs (post-CW):

| Run (SHA)                | createdAt → updatedAt | Run wall | Saturation? |
|--------------------------|-----------------------|----------|-------------|
| CW5 (`23eb430e`)         | 06:50:46 → 07:36:11   | **45m 25s** | yes — libsql queued 27m |
| DU7 proof (`32951ee7`)   | 05:08:59 → 05:32:33   | **23m 34s** | no |
| CA3 hotfix 5 (`0321c728`) | 04:33:49 → 05:02:13  | **28m 24s** | no |

The CW0 baseline was anchored on the DU7 proof (23m 34s) because
Swatinem cache was hot from the immediately-prior CA hotfix run on
near-identical code. That made CW look like a ~5% improvement
against the realistic baseline (CA3 hotfix 5: 28m 24s → 27m 5s).
Post-CW, the run *floor* is ~25m and the *ceiling* under
saturation is ~45m. Neither side is acceptable for a sub-15m
target. PW resets the goalposts to a measured-p95 contract rather
than a single-run wall.

### CW5 critical-path decomposition (45m 25s = pole)

```
00:00  ui-artifacts                          27s  ✓
00:30  warm-sccache (needs ui-artifacts)   6m 16s ─┐
06:46  └─→ harness shards (×11)            ~11m   │
06:46  └─→ coverage shards (×3)            ~14m   │
20:46      └─→ coverage reducer            5m 47s │
       (parallel lateral lanes:)                  │
00:00  rust-clippy                          7m 20s│
00:00  rust-workspace-tests (×3)            ~10m  │
00:00  rust-runtime-tests                   10m 39s
00:00  external-provider postgres           9m 25s
00:00  external-provider mysql              9m 8s
00:00  external-provider libsql           17m 45s ◀ saturation-queued to 07:18
26:33  coverage reducer ends
45:20  rust-gate-summary (needs libsql)    2s
45:22  end
```

The two compounding effects:

- **Structural floor**: even with zero saturation, the coverage
  track is 26m 40s and the harness track is 17m 33s. PR wall is
  `max(gate=18m, coverage=26m, harness=17m)`. Coverage dominates.
- **Saturation tail**: ~24 simultaneous jobs at peak (1st wave +
  2nd wave overlap) exceeds the runner allocation budget. The
  unlucky job that queues last (CW5: libsql; sometimes a coverage
  shard; sometimes the engine harness) drags the wall up by
  whatever queue time it ate.

### Per-pole gap mechanisms

**Pole 1 — `libsql` is 2× postgres/mysql for identical wall shape.**
The container is `ghcr.io/tursodatabase/libsql-server:latest`, not
a pinned tag. Every run pulls a fresh image (GHCR layer manifest
churn, no docker-image cache) and the `--enable-namespaces` mode
adds per-test namespace setup overhead. Postgres and mysql use
official images (`postgres:16`, `mysql:8.4`) which are heavily
cached by the GitHub Actions runner image itself. The actual test
work — `make test-external-providers` filtered to libsql — is not
the pole; the cold container pull + namespace init is.

**Pole 2 — coverage sits on the PR critical path but is not on the
merge gate.** `rust-gate-summary` needs format/clippy/deny/runtime-
tests/workspace-tests/external-provider-tests. Coverage is
parallel but not blocking the gate. It blocks the *workflow wall*
because `actions/checkout`-based PR check waits for all jobs to
complete. Coverage on `main` is the contract we actually care
about (instrumented binary, lcov upload, dashboard). Coverage on
PR is best-effort signal at high wall cost.

**Pole 3 — concurrent-job count exceeds GitHub Actions allocation
budget.** The org's effective concurrent ubuntu-runner ceiling is
the limiting factor. The 1st wave starts 13 jobs at t=0:18s. The
2nd wave (harness ×8 + coverage ×3) starts at t=6m46s when
warm-sccache completes, *adding* 11 more jobs. Net simultaneous:
~24. When that exceeds the ceiling, one job queues. On CW5 the
queued job was libsql; we don't get to pick.

## Scope

### In scope

- `.github/workflows/ci.yml` — pole-attack edits (libsql image
  pin, concurrency cap, coverage extraction or gating)
- `.github/workflows/coverage.yml` — new file if PW2 extracts the
  coverage track instead of inline-gating it
- `.github/actions/setup-rust-cached/action.yml` — only if PW4
  measurement reveals a fixable cache miss pattern
- `docs/operating/ci-pr-wall.md` — new canonical contract page
- `docs/operating/ci-modernization.md` — cross-reference link
  added; no content move
- `docs/plans/proof/ci-pr-wall-sub-15/` — proof bundles per item
- `scripts/verify-ci-pr-wall-sub-15.sh` — verifier script
- `CLAUDE.md` — routing entry promoted at closeout

### Out of scope

- `release.yml`, `linux-distribution-release.yml`, `linux-packages.yml`,
  `apt-repo.yml`, `copr-srpms.yml`, `desktop-ui.yml`, `codeql.yml`,
  `node-compat-nightly.yml`, `verify-nimbus-crun-patch.yml`. PW is
  about the PR wall on `ci.yml` only. Release acceleration is its
  own future plan (Windows release-build pole, machine-os 150m
  build).
- Adding new test surfaces or expanding harness corpus.
- Sharding reductions or increases beyond what's required to land
  the wall target. The CW shard counts (3-way workspace, 4-way
  server harness, 3-way coverage, 3-way provider matrix) all
  pay for themselves; PW does not touch them except for PW4
  warm-sccache.
- Self-hosted runners. Standard `ubuntu-24.04` only.
- Larger GitHub-hosted runner sizes. Standard runners only.
- Changing the gate's required-checks list (still
  format/clippy/deny/runtime-tests/workspace-tests/external-provider).
- Removing harness from PR. Harness lanes are merge-required for
  engine/server/runtime/storage surfaces; PW keeps them.

## Authorization model

PW touches `.github/workflows/*.yml` and the canonical contract
docs. Surface is identical to CW/CA/CC/CM, which the user has
pre-authorized for autonomous push to `main` (see
`feedback_cw_plan_autonomous_mode.md`,
`feedback_ca_plan_autonomous_mode.md` in memory).

**PW is pre-authorized for autonomous push to `main`** for items
PW0..PW6 once the plan is approved. Specifically:

- Editing `ci.yml`, `coverage.yml`, `setup-rust-cached/action.yml`
- Creating `docs/operating/ci-pr-wall.md`
- Updating `CLAUDE.md` routing
- Committing per-item with execution-log SHA backfill
- Pushing to `main` without a PR (pre-launch repo, no PR contract)

**PW is NOT authorized to:**

- Touch `release.yml` or any release-pipeline workflow
- Change required-status-checks rules on the GitHub branch (not
  in git)
- Remove the merge gate or any of its `needs:` entries
- Skip pre-commit / pre-push hooks (`--no-verify`)
- Force-push to `main`
- Amend committed commits

**Coverage extraction (PW2) is reversible.** If post-PW2 main
runs reveal coverage regressions blocking releases, PW2 ships a
documented "rollback recipe" (single-commit revert + dashboard
re-link). Same pattern as CW3's per-provider split could have
been rolled back to the old serial provider job.

## Wall-time math

### Baseline (post-CW, today)

```
Gate path   : max(clippy 7m, deny 5m, runtime 11m, workspace 10m, provider 18m)
            = 18m   ← libsql-bound
Cov  path   : ui-artifacts 30s + warm-sccache 6m16s + cov_rest 14m10s + reducer 5m47s
            = 26m 40s
Harn path   : ui-artifacts 30s + warm-sccache 6m16s + harness_max 11m37s
            = 18m 25s
Wall (good) : max(gate, cov, harn) = 26m 40s
Wall (sat)  : 45m+ when any 2nd-wave job queues
```

### After PW1 (libsql image pin + cache lane)

```
Gate path   : max(7m, 5m, 11m, 10m, ≤10m libsql) = 11m
Cov  path   : unchanged (26m 40s)
Harn path   : unchanged (18m 25s)
Wall (good) : 26m 40s — coverage still dominates
Saturation  : reduced — libsql leaves the runner pool earlier
```

### After PW1 + PW2 (coverage off PR)

```
Gate path   : 11m
Cov  path   : nightly + main-only (off PR wall)
Harn path   : 18m 25s
Wall (good) : max(11m, 18m 25s) = 18m 25s
Saturation  : ~13 simultaneous jobs at peak (was ~24); within budget
```

### After PW1 + PW2 + PW3 (concurrency cap)

```
Same wall floor as above (18m 25s). Tail variance collapses;
p95 ≈ p50 within ±1m. **PR wall target ≤ 20m achievable; not yet
≤ 15m.**
```

### After PW1 + PW2 + PW3 + PW4 (warm-sccache retirement, if data
supports)

```
Gate path   : 11m
Harn path   : ui-artifacts 30s + harness_max ≤ 13m = 13m 30s
              (depends on Swatinem v2 target-cache hit rate)
Wall (good) : max(11m, 13m 30s) = 13m 30s  ← sub-15
Wall (cold) : harness_max ≈ 16m on Swatinem cache miss
              → would degrade gracefully to ~17m, still ≤ today
```

PW4's data-driven gate: if PR-shaped runs over a 7-day window
show Swatinem target-cache hit rate ≥ 85% on harness lanes, PW4
retires warm-sccache. Otherwise PW4 documents the cache-miss
pattern, keeps warm-sccache, and declares PW success at the PW3
floor (≈18m wall, p95 ≤ 20m). Honest exit either way.

## Ledger

| Item   | Scope                                                   | Verifier conditions touched | Authorization |
|--------|---------------------------------------------------------|-----------------------------|---------------|
| PW0    | Scaffold plan + verifier + baseline proof               | 1–3                         | autonomous    |
| PW1    | Pin libsql image tag + docker-image cache lane          | 4                           | autonomous    |
| PW2    | Extract coverage track to nightly + main-only workflow  | 5                           | autonomous    |
| PW3    | Add workflow-level concurrency cap                      | 6                           | autonomous    |
| PW4    | Swatinem cache-hit measurement → retire-or-document warm-sccache | 7              | autonomous; data-gated |
| PW5    | Sub-15 PR wall green proof (3 runs)                     | 8                           | autonomous    |
| PW6    | Closeout — promote contract, archive plan, update routing | 9–10                       | autonomous    |

### PW0 — Scaffold plan + verifier + baseline proof

Land this document plus `scripts/verify-ci-pr-wall-sub-15.sh`
(failing on conditions 4–10 by design, passing on 1–3) plus a
baseline proof bundle.

Deliverables:

- `docs/plans/ci-pr-wall-sub-15-plan.md` (this file)
- `scripts/verify-ci-pr-wall-sub-15.sh`
- `docs/plans/proof/ci-pr-wall-sub-15/pw0-baseline.md`

`pw0-baseline.md` records:

- Last 5 `main` runs with wall, gate, longest-job
- Per-job duration histogram across those 5 runs
- libsql duration p50 + p95 vs postgres/mysql
- Swatinem cache-hit signal if extractable from logs
- Concurrent-job count at peak (max simultaneous "in_progress")

The verifier should pass conditions 1, 2, 3 immediately so
`/goal ci-pr-wall-sub-15` is satisfiable from day one.

### PW1 — Pin libsql image + add docker-image cache lane

Two `libsql-server:latest` references exist in `ci.yml` today:

- Line 324 in `external-provider-tests` (gate path)
- Line 772 in the coverage `libsql` shard (full-wall path)

PW1 pins both. PW2 will move the second to `coverage.yml` — the
pin moves with it.

Two changes per usage:

1. Replace `ghcr.io/tursodatabase/libsql-server:latest` with a
   specific pinned tag (the latest known-good `vX.Y.Z` from
   `tursodatabase/libsql-server` releases, recorded in the proof
   bundle). Pin docs follow the same SHA-pinning convention CM5
   established for non-`actions/*` references — add a
   `# vX.Y.Z` comment on the line above.

2. Add `actions/cache@<sha>` for the docker image layer:
   `docker save <pinned> | gzip > /tmp/libsql.tar.gz` cached
   between runs, `docker load < /tmp/libsql.tar.gz` on hit. Same
   pattern CC4 used for the UI artifacts leader job.

Verifier condition 4: `ci.yml` external-provider matrix `libsql`
case uses a pinned image tag (regex must NOT match `:latest`).

Expected impact: libsql 18m → ≤ 10m. Gate floor 18m → 11m.

Proof bundle: `pw1-libsql-pin.md` records 3 consecutive `main`
runs post-pin with libsql duration recorded.

### PW2 — Extract coverage track to nightly + main-only

Move the coverage job tree off the PR critical path. Two
candidate shapes:

- **Shape A**: extract to `.github/workflows/coverage.yml` with
  `on: { schedule: { cron: "0 6 * * *" }, push: { branches:
  [main] } }`. Inline coverage jobs deleted from `ci.yml`.
- **Shape B**: keep coverage jobs in `ci.yml` but gate them with
  `if: github.ref == 'refs/heads/main' || github.event_name ==
  'schedule'`. PR runs skip them entirely.

Recommendation: **Shape A**. Reasons:

- `ci.yml` is already 937 lines and crowded
- Coverage has its own observability target (dashboard,
  `$GITHUB_STEP_SUMMARY` lcov badge from CA5) that benefits from
  workflow-level isolation
- A standalone `coverage.yml` makes the contract explicit:
  "coverage is a main + nightly artifact, not a PR gate"
- Failure isolation: a coverage regression no longer fails the
  `ci.yml` workflow on PRs, only its own workflow on main

Shape A scope:

- New file `.github/workflows/coverage.yml` with the coverage
  shards (server/engine/rest) and reducer, on
  `schedule + push:main + workflow_dispatch`
- Delete coverage jobs from `ci.yml`
- Update `rust-gate-summary.needs:` if coverage was ever listed
  there (it isn't today — confirm in proof bundle)
- Update the CA5 `$GITHUB_STEP_SUMMARY` lcov badge emitter to
  read from the new workflow's artifact
- `docs/operating/ci-modernization.md` "Coverage and release
  acceleration" section updated with a pointer to PW2

Verifier condition 5: `coverage.yml` exists with `schedule` +
`push.branches: [main]` triggers AND `ci.yml` no longer contains
`name: Coverage shard` or `name: Coverage reducer`.

Expected impact: PR wall ceiling drops from 26m 40s → 18m 25s.

Proof bundle: `pw2-coverage-extract.md` records the file diff
summary, 3 PR-shaped runs (push to a throwaway PR branch) showing
coverage jobs absent, and 1 nightly run showing coverage jobs
present and green.

### PW3 — Workflow-level concurrency cap (refine, not add)

`ci.yml` already carries a top-level `concurrency:` block:

```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
```

The bug: `cancel-in-progress: true` cancels `main` runs too. When two
`main` commits land back-to-back, the first run is killed before its
Swatinem cache-save and sccache stats-save effects land. That
silently corrodes the cache hit rate PW4 depends on.

PW3 flips this to branch-conditional cancellation:

```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}
```

This serves two purposes:

- **Per-branch cancellation on push** — if you push twice in a
  row to the same PR branch, the first run cancels and the
  second proceeds. Cuts wasted CI minutes; eliminates "two
  parallel runs racing for the same runner pool" pattern.
- **No cancellation on main** — `main` runs always complete so
  the cache-save side effects (Swatinem cache, sccache stats,
  llvm-cov artifact) land deterministically.

The concurrency cap by itself doesn't reduce simultaneous job
count *within* a single workflow run — that's a per-job
allocation pattern controlled by GitHub. But it does kill the
common cause of saturation: two-runs-deep contention from a
quick succession of pushes.

If saturation persists after PW3 ships, PW3 has a secondary
move: add `needs: [warm-sccache]` to the third coverage shard
and the 4th server harness shard, staggering the 2nd wave so
peak simultaneous count drops from ~24 to ~20. This is
"counterintuitive serialization" — slowing the slowest lane to
shrink the peak. Only land it if PW3.1 (the concurrency-block
alone) leaves a measurable saturation tail.

Verifier condition 6: `ci.yml` has a top-level `concurrency:`
block with `group:` referencing `github.ref` and a
`cancel-in-progress:` directive.

Expected impact: p95 wall variance collapses to p50 ± 1m on PR
branches. The CW5-style 45m tail disappears.

Proof bundle: `pw3-concurrency.md` records the diff plus a
quick A/B: 3 PR-branch double-pushes pre-cap and post-cap, with
the cancelled-run count and saved-CI-minutes recorded.

### PW4 — Swatinem cache measurement → retire-or-document warm-sccache

PW4 is the only data-gated item. Two phases:

**PW4a — Measure.** Sample 7 days of post-PW3 PR runs. For each
harness/coverage/workspace-tests job, extract from the action
log:

- Swatinem `Cache restored from key: <key>` line (hit) or
  `Cache not found for keys: <keys>` line (miss)
- sccache stats post-job (compilation count vs cache hits)
- Total job duration

Aggregate by job name. Hit rate = restored / (restored + missed).

If overall Swatinem hit rate on harness lanes ≥ 85% and per-job
duration on hits is ≤ 13m, proceed to PW4b. Otherwise jump to
PW4c.

**PW4b — Retire warm-sccache.** Delete the `warm-sccache` job
from `ci.yml`. Remove `needs: [warm-sccache]` from `harness:`.
Land it. Verify next 3 `main` runs don't regress past 17m.

**PW4c — Keep warm-sccache.** Document the cache-miss pattern in
`docs/operating/ci-pr-wall.md`. Note specific job-name + cache-
miss-rate signature so future PW-N can attack the cache invalidation
seam directly. Declare PW success at the PW3 floor (~18m wall).

Verifier condition 7: `ci.yml` either has no `warm-sccache:` job
(PW4b path) OR contains a comment block on the `warm-sccache:`
job referencing `pw4c-warm-sccache-retained.md` with a measured
cache-miss explanation (PW4c path).

Proof bundle: `pw4-measurement.md` + (`pw4b-retirement.md` OR
`pw4c-warm-sccache-retained.md`).

### PW5 — Sub-15 PR wall green proof

3 consecutive PR-branch runs measured post-PW4. For each:

- Wall (createdAt → updatedAt of the workflow run)
- Critical-path job and its duration
- Saturation marker: max simultaneous "in_progress" job count

PW5 passes if:

- Each of the 3 runs is on a PR branch with a real code change
  (not docs-only; harness must trigger)
- Each run wall ≤ 15m if PW4b shipped, OR ≤ 18m if PW4c shipped
- No run shows queue-time tail (each job's `started - created`
  delta < 60s)

Verifier condition 8: `pw5-green-proof.md` exists with 3 runs
documented and verifier-extractable wall numbers.

Proof bundle: `pw5-green-proof.md` records the 3 runs with
links to the GitHub Actions UI.

### PW6 — Closeout

- Promote `docs/operating/ci-pr-wall.md` to canonical contract
  with the wall target, the per-pole attack summary, and the
  retain/retire warm-sccache decision (whichever PW4 produced)
- Update `CLAUDE.md` routing entry under
  `docs/operating/ci-modernization.md` heading to point at
  PW closeout
- Move plan to `docs/plans/archive/ci-pr-wall-sub-15-plan.md`
- Update `docs/plans/README.md` to reflect the archive

Verifier conditions 9 + 10: both files exist at their archive
paths and `CLAUDE.md` mentions `ci-pr-wall-sub-15-plan` in the
"CI infrastructure modernization" routing block.

Proof bundle: `pw6-closeout.md` with the plan-archive diff.

## Completion Gate

`scripts/verify-ci-pr-wall-sub-15.sh` exits 0 iff all of:

1. `docs/plans/ci-pr-wall-sub-15-plan.md` exists at the active or
   archived path with `Status: active|complete` frontmatter.
2. `scripts/verify-ci-pr-wall-sub-15.sh` exists and is
   executable.
3. Each ledger item PW0..PW6 has an entry in the Execution Log
   with a 40-char hex SHA.
4. `.github/workflows/ci.yml` external-provider-tests matrix
   contains a libsql case with a pinned image tag (regex must
   NOT match `:latest`); the previous line must contain a
   `# vX.Y.Z` version-name comment.
5. `.github/workflows/coverage.yml` exists with `schedule:` AND
   `push.branches: [main]` triggers AND `.github/workflows/
   ci.yml` does NOT contain any line matching `name: Coverage
   shard` or `name: Coverage reducer`.
6. `.github/workflows/ci.yml` top-level `concurrency:` block has
   `group:` referencing `github.ref` and `cancel-in-progress:` that
   either evaluates to `false` on `refs/heads/main` or is the literal
   expression `${{ github.ref != 'refs/heads/main' }}`. A bare
   `cancel-in-progress: true` fails the condition (the present-day
   shape) because it kills main's cache-save side effects.
7. `.github/workflows/ci.yml` either does not contain a
   `warm-sccache:` job (PW4b path) OR contains the
   warm-sccache job with a comment line referencing
   `pw4c-warm-sccache-retained.md` and a documented cache-miss
   rationale.
8. `docs/plans/proof/ci-pr-wall-sub-15/pw5-green-proof.md`
   exists and contains 3 run-id lines matching
   `^Run: \d{10,12}` with a wall value parsable as `≤ 15m` (or
   `≤ 18m` if condition 7 takes the PW4c path).
9. `docs/operating/ci-pr-wall.md` exists with sections
   `## Target`, `## Pole attacks`, `## Retain/retire warm-sccache`.
10. `CLAUDE.md` "Routing By Work Type" contains an explicit
    `ci-pr-wall-sub-15-plan.md` mention in the CI modernization /
    PR-wall block.

## Risks

- **Coverage extraction breaks the lcov dashboard link.** PW2
  produces artifacts under a new workflow name; any downstream
  consumer (`docs/operating/ci-modernization.md` link, README
  badge, internal dashboard query) must be re-pointed. Mitigation:
  PW2 proof bundle enumerates every reference to the old workflow
  name found via grep, and PW2 updates them all in the same
  commit.
- **libsql tag pin lands on a stale image with a known bug.** A
  pinned tag that's older than `:latest` may carry a server-side
  bug `:latest` already fixed. Mitigation: PW1 picks the most
  recent `vX.Y.Z` GHCR tag at PW1 land time and records the date.
  Subsequent freshness bumps are a standalone follow-up (3-month
  cadence is enough).
- **Concurrency cap masks a runner-pool problem.** If the org's
  runner pool is genuinely undersized for the team, PW3 makes
  the symptom go away (fewer parallel runs racing) but doesn't
  fix the underlying capacity. Mitigation: PW3 proof bundle
  records max simultaneous job count post-cap; if it still hits
  the pool ceiling on legitimate single-PR runs, that's a
  capacity escalation, not a plan failure.
- **warm-sccache retirement causes a cache-miss storm on cold
  branches.** PW4b assumes Swatinem v2 target-cache hits at high
  rate, but a fresh branch off main with significant churn will
  miss. Mitigation: PW4b's first 3 main runs are the proof gate.
  If wall regresses past 17m on any of them, PW4 falls back to
  PW4c (keep warm-sccache, declare success at PW3 floor).
- **PW5 green proof anchors on lucky runs.** 3 consecutive
  ≤15m runs could be Swatinem-cache-luck. Mitigation: PW5
  documents the 7-day p95 wall after PW4 lands, not just 3
  runs. The verifier checks the 3-run condition; the proof
  bundle records the broader window.

## Non-goals

- **Not a release.yml plan.** Release acceleration (Windows
  vendored OpenSSL, machine-os 150m build) is a separate future
  plan. PW does not touch `release.yml` or its matrix.
- **Not a runner-pool capacity plan.** PW3 caps concurrency to
  what GitHub gives us; if that's structurally too small we
  escalate to org owners, not edit YAML.
- **Not a sharding-strategy plan.** CW's 3/4/3 shard counts
  stand. PW4 retires *one* prelude job; it does not redivide
  shards.
- **Not a coverage-target plan.** PW2 moves coverage off PR; it
  does not change coverage gates, lcov thresholds, or the
  CA-archive coverage contract. Coverage continues to gate
  releases on main.
- **Not a harness-corpus plan.** Harness stays on PR; PW does
  not add, remove, or reweight harness lanes. The verification-
  harness contract is owned by the harness plan, not PW.
- **Not a self-hosted-runner plan.** Standard `ubuntu-24.04`
  only. Self-hosted runners introduce capacity, security, and
  maintenance commitments PW is not willing to take on.

## Execution Log

(Populated as items land. Each row records the commit SHA that
shipped the item plus a one-line "what landed" note.)

| Item | SHA | Note |
|------|-----|------|
| PW0  | bf5dbfeb29d78ece9740ec532f4584e911e86298 | scaffold + verifier + baseline proof |
| PW1  | e79bbfc54a6841bc08f8585b057a876b5f650d8b | libsql tag pin + docker-image cache |
| PW2  | 333d398eb2714fcc9039653729e2d2753d445d40 | coverage.yml extraction; PR coverage gone |
| PW3  | 4ebe916dd8609dec2a5e3ea1b93646273696082b | concurrency cap: branch-conditional cancel |
| PW4  | 09d3a290c20b257d1488815282c3aef11b06f614 | warm-sccache retained (PW4c path) |
| PW5  | TBD | sub-15 PR wall green proof bundle |
| PW6  | TBD | contract promoted, plan archived, routing updated |

## Quick links

- Previous wave (closed 2026-05-23):
  [`docs/plans/archive/ci-wall-acceleration-plan.md`](archive/ci-wall-acceleration-plan.md)
- Coverage baseline (closed 2026-05-22):
  [`docs/plans/archive/coverage-acceleration-plan.md`](archive/coverage-acceleration-plan.md)
- Caching baseline (closed 2026-05-22):
  [`docs/plans/archive/ci-caching-canonicalization-plan.md`](archive/ci-caching-canonicalization-plan.md)
- Modernization baseline (closed 2026-05-22):
  [`docs/plans/archive/ci-modernization-plan.md`](archive/ci-modernization-plan.md)
- Canonical CI contract:
  [`docs/operating/ci-modernization.md`](../operating/ci-modernization.md)
