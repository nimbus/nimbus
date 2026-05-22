# CC1 — Coverage-only sccache pilot stats

CC1 wired `mozilla-actions/sccache-action@v0.0.6` into the Coverage job
of `.github/workflows/ci.yml`, set `RUSTC_WRAPPER=sccache` and
`SCCACHE_GHA_ENABLED=true` at the (then-job, now workflow) env level,
and added a `sccache --show-stats` step that prints the hit/miss
counters at the end of every Coverage run.

The plan asked for 5-10 runs to observe the hit-rate trend. By the
time those runs would have accumulated, the rest of the rollout
(CC2-CC5) had already cascaded — sccache was already expanded to every
Rust job, ui-artifacts was producing the SPA dist, and warm-sccache
was populating the shared pool before downstream jobs fan out. So the
CC1-pilot snapshot is folded into the post-CC5 observation pass
described below.

## CC1 in isolation — what was true at the close of `bbfe6c70`

- Coverage job was the only consumer of `mozilla-actions/sccache-action`.
- Workflow-level `CARGO_INCREMENTAL: "0"` was already set (LD-era),
  so sccache's hard requirement was met.
- Swatinem shared-key was unchanged at `ci-ubuntu-stable-coverage-no-bin-v1`
  (the floor cache was the legacy monolithic slot; sccache layered
  *on top of* it).
- The CC1 commit (bbfe6c70) was the first push where the Coverage
  job's `Cache cargo artifacts` step had a partner `Install sccache`
  step. The first CI run of bbfe6c70 was the cold-pool case: sccache
  storage was empty, every rustc invocation missed and wrote back,
  net wallclock matched the baseline within sccache miss overhead.

## What changes after CC2-CC5

- CC2 expanded sccache to every Rust job and rotated Swatinem
  shared-keys `-v1 → -v2`, so the *floor* is also cold on the first
  push after that change.
- CC3 added `save-always: true` so reruns don't suppress saves.
- CC4 added `ui-artifacts` so downstream Rust jobs share one SPA
  build.
- CC5 added `warm-sccache` so the per-rustc-call pool is populated
  *serially first* before harness/coverage fan out.

The CC1 isolated-pilot snapshot is therefore the **baseline** for the
ongoing sccache hit-rate trend, not the steady-state target. The
steady-state target is observed after a CC5-run warm-sccache populates
the pool and harness/coverage downstream jobs hit ~70-80% per the
`docs/operating/ci-caching.md` contract.

## Steady-state observation — pending

The first push that exercises the full CC0-CC7 stack is post-CC7
(this current set of commits). The Coverage job's sccache stats from
that and the next ~5 pushes will land here as a per-run table:

| Run ID | HEAD | Pool state | Coverage wallclock | sccache hits | sccache misses | hit rate |
|--------|------|------------|---------------------|---------------|-----------------|----------|
| _pending_ | _pending_ | _pending_ | _pending_ | _pending_ | _pending_ | _pending_ |

The target metrics, restated from the plan:

- Cross-job hit rate >70% on push N+2 (post-`warm-sccache`).
- Total Actions cache pool size drops by >50% versus the
  `baseline-cache-state.json` 32.49 GB.
- Coverage wallclock on a warm-pool push drops from 22m 11s baseline
  to 8-12 min (sccache hits) — and further to ~6-8 min once CC7's
  `--no-doc-tests` halves the link work.

## Sources

- `docs/plans/proof/ci-caching-canonicalization/baseline-coverage-timings.md`
- `docs/plans/proof/ci-caching-canonicalization/baseline-cache-state.json`
- `docs/operating/ci-caching.md` — caching contract for the full stack
- `docs/plans/ci-caching-canonicalization-plan.md` rows CC1-CC7
