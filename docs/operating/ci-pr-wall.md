# CI PR-wall contract

This page is the canonical contract for what "the CI PR wall" means
in this repo: what it gates, what its target is, how the four poles
are attacked, and the retain-vs-retire decision on warm-sccache.

## Target

PR wall p95 ≤ **18 minutes**.

The 15-minute target the original CI PR-Wall Sub-15 plan
(`docs/plans/archive/ci-pr-wall-sub-15-plan.md`) aimed for is
reachable only on the PW4b path (retire warm-sccache). That path is
deferred — see "Retain vs retire warm-sccache" below — so the
operational target is 18m. The verifier
(`scripts/verify-ci-pr-wall-sub-15.sh` condition 8) flips between
15m and 18m automatically based on whether `warm-sccache:` is
present in `ci.yml`.

The PR wall is the **rust-gate-summary** job — that is the merge
gate. Its `needs:` list is the operational contract:

```yaml
rust-gate-summary:
  needs:
    - rust-format
    - rust-clippy
    - deny
    - rust-runtime-tests
    - rust-workspace-tests
    - external-provider-tests
```

Anything not on that list is **not on the PR wall**, even if it
happens to be in `ci.yml`. Coverage is the canonical example: it
runs in its own workflow (`.github/workflows/coverage.yml`) on
`push.main + schedule + workflow_dispatch`, never on
`pull_request:`.

## Pole attacks

The CI PR-Wall Sub-15 plan identified four wall-time poles on the
PR critical path (baseline: CW5 at `23eb430e`, wall 45m 25s).

### Pole 1 — libsql gate dominance (was 17m 45s)

The external-provider matrix has three legs (mysql, postgres,
libsql). libsql ran 1.94× the mysql/postgres baseline because
`ghcr.io/tursodatabase/libsql-server:latest` was pulled fresh on
every run.

**PW1 attack:** pin the image tag and cache the docker layer.

- Image pinned to `ghcr.io/tursodatabase/libsql-server:v0.24.33`
  with a sibling `# v0.24.33` comment near each usage.
- Three steps before "Start libsql provider fixture":
  1. `actions/cache@v5` on `/tmp/libsql-image.tar.gz` keyed
     `libsql-image-v0.24.33`.
  2. `docker load --input /tmp/libsql-image.tar.gz` on cache hit.
  3. `docker pull` + `docker save | gzip` on cache miss.

Cold cache: pull + save ≈ pull + 2s. Warm cache: load ≈ 5s.

The tag is chosen by probing `ghcr.io/v2/tursodatabase/libsql-server/tags/list`
and taking the most recent `v0.24.*`. Note: the upstream GitHub
release page lists tags that do not exist on GHCR (e.g. `v0.24.32`
404s individually). Always probe GHCR directly when refreshing.

**Expected impact:** libsql 17m 45s → ≤ 10m on warm runs.

### Pole 2 — Coverage on PR critical path (was 26m 40s)

The coverage track had the following critical path:

```
ui-artifacts (0m 27s) → warm-sccache (6m 16s)
  → coverage shard rest (max ~14m 10s) → coverage-reduce (5m 47s)
```

= 26m 40s, **none of which gates merge** (rust-gate-summary does
not need coverage).

**PW2 attack:** extract coverage to its own workflow.

- New file `.github/workflows/coverage.yml` containing the
  coverage matrix shards (server / engine / rest), the
  coverage-reduce job, and self-contained leader jobs
  (`ui-artifacts`, `warm-sccache`) so coverage runs do not
  depend on `ci.yml`.
- Triggers: `push: branches: [main]` + `schedule` (weekly) +
  `workflow_dispatch`. No `pull_request:`.
- Coverage no longer runs on PRs.

The libsql v0.24.33 pin + cache lane (PW1) moves with the coverage
job into `coverage.yml`.

**Expected impact:** PR wall ceiling 26m 40s → off the path
entirely. Indirectly frees ~4 concurrent runner slots from each PR
push, which mitigates pole 3.

### Pole 3 — Concurrent-runner saturation (was 27m queue wait)

On CW5, libsql queued at +0m 18s but didn't start running until
+27m 35s, behind the 11-job second wave (harness + coverage). Peak
simultaneous job count was 24. Without the saturation, CW5 would
have been ~27m instead of 45m.

There were two contributors:

1. **PR runs producing too many concurrent jobs.** PW2's coverage
   extraction removes ~4 from the PR side.
2. **Back-to-back main pushes cancelling each other mid-flight.**
   `ci.yml` already had a top-level `concurrency:` block, but it
   used `cancel-in-progress: true` unconditionally. A cancelled
   main run abandons its cache-save side effects (sccache GHA
   cache, Swatinem target cache, libsql-image cache), leaving the
   next run cold.

**PW3 attack:** flip `cancel-in-progress` to branch-conditional.

```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}
```

PR branches still cancel themselves on rapid pushes (the
expression evaluates to `true`). Main runs always complete
(`false`), preserving cache-save side effects.

The same pattern ships in `coverage.yml` (PW2 set it up that way
from the start, so a cancelled main coverage run cannot abandon
the lcov.info upload and leave Codecov per-commit gaps).

**Expected impact:** main cache hit rate stabilises; libsql 27m
queue wait drops as PR runs free up runners.

### Pole 4 — warm-sccache prelude (was 6m 16s)

The `warm-sccache:` job (added in CC5) rustc's the workspace once
to populate sccache so downstream parallel Rust jobs hit instead
of all cold-compiling identical deps.

It sits at the top of the PR critical path with no parallel
opportunity (harness shards depend on it). After CC9 + the
Swatinem v2 `save-always: true` from the caching baseline, the
**theoretical** opportunity exists to retire it — if Swatinem hits
≥ 85% across all gate-path jobs, the warm pass is redundant.

This is the data-gated decision PW4 makes. See the next section.

## Retain vs retire warm-sccache

The PW4 decision branches:

- **PW4b (retire):** delete the `warm-sccache:` job. PR wall floor
  drops to ~13m 30s; the 15m target becomes achievable.
- **PW4c (retain):** keep `warm-sccache:` with a comment block
  referencing
  `docs/plans/proof/ci-pr-wall-sub-15/pw4c-warm-sccache-retained.md`.
  PR wall floor stays at ~18m; success target is 18m.

The verifier accepts either path. Condition 8 reads the
`warm-sccache:` presence in `ci.yml` and flips the wall threshold
between 15m (retired) and 18m (retained).

**The current state is PW4c (retained).** Rationale:

CW5 data shows uneven Swatinem hit rate across gate-path jobs:

| Job                              | Swatinem | sccache (Rust) |
|----------------------------------|----------|----------------|
| Storage Verification Harness     | hit      | 76.92% |
| External Provider Tests (mysql)  | hit      | (high) |
| External Provider Tests (libsql) | **miss** | **0.00%** |

The libsql shard misses Swatinem (it shows `Cache Key:` but no
following `Cache hit for:` / `Restored from cache key "..." full
match: true`), and consequently sccache reports 0% Rust hits on
that job. Without `warm-sccache:` to pre-fill the per-rustc-call
sccache, the libsql shard would pay full cold-compile cost on
every PR.

The root cause of the libsql Swatinem miss is unknown. Candidates:

- Matrix-leg-specific cache key hashing.
- `--features libsql` (if present in the shard's cargo command)
  reshaping the dependency tree.
- Workspace state side effects (mtimes, generated content) from
  the libsql Docker fixture.

A future PW-style wave can investigate this seam directly. Until
that wave lands, **do not retire warm-sccache**.

## Verifier

`scripts/verify-ci-pr-wall-sub-15.sh` enforces the contract above
with ten conditions. The path-flipping conditions are:

- Condition 7: warm-sccache state (retired OR retained-with-pointer)
- Condition 8: wall threshold (15m if retired, 18m if retained)

All other conditions are unambiguous: plan exists with Status
frontmatter, verifier script present, execution log SHAs recorded,
libsql refs pinned, coverage extracted, concurrency cap correct,
PW5 proof bundle present with three runs, this doc present, and
CLAUDE.md routing references the plan.

## Quick links

- Active baseline: this file
- Archived plan: [`docs/plans/archive/ci-pr-wall-sub-15-plan.md`](../plans/archive/ci-pr-wall-sub-15-plan.md)
- Proof bundle directory:
  [`docs/plans/proof/ci-pr-wall-sub-15/`](../plans/proof/ci-pr-wall-sub-15/)
  - `pw0-baseline.md` — 5-run sample + CW5 timeline + pole evidence
  - `pw1-libsql-pin.md` — tag selection + cache lane
  - `pw2-coverage-extract.md` — extraction rationale + diff summary
  - `pw3-concurrency-cap.md` — cancel-in-progress flip
  - `pw4c-warm-sccache-retained.md` — measurement + deferred work
  - `pw5-green-proof.md` — 3 post-PW4 main runs ≤ 18m
- Prior wave (closed 2026-05-23):
  [`docs/plans/archive/ci-wall-acceleration-plan.md`](../plans/archive/ci-wall-acceleration-plan.md)
- Caching baseline (closed 2026-05-22):
  [`docs/plans/archive/ci-caching-canonicalization-plan.md`](../plans/archive/ci-caching-canonicalization-plan.md)
- Coverage baseline (closed 2026-05-22):
  [`docs/plans/archive/coverage-acceleration-plan.md`](../plans/archive/coverage-acceleration-plan.md)
- CI modernization contract:
  [`docs/operating/ci-modernization.md`](ci-modernization.md)
