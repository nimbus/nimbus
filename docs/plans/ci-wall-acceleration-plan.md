# CI Wall Acceleration Plan (CW)

The CA plan closed with the Coverage critical path on `main` collapsed
(24m27s → ~8m for the coverage path). But the CI workflow wall did
not see a proportional improvement: latest post-CA run on `32951ee7`
took **23m34s**, only marginally faster than pre-CA heavy runs
(`a4c52f5e` at 25m). The reason is that Coverage is no longer the
pole. The new poles are verification-harness jobs (Server Verification
Harness at 12.7m, Engine at 11.0m), `Rust Workspace Tests` (15.7m),
and `External Provider Integration Tests` (14.6m).

The CW plan attacks those poles directly.

## Why this plan exists

Latest CI run on `main` (SHA `32951ee7`, CW0 baseline) — total wall
**23m34s**, per-job breakdown:

| Job                                  | Duration | Critical-path role |
|--------------------------------------|----------|--------------------|
| Rust Workspace Tests                 | **15.7m** (lateral pole) | not gated on warm-sccache |
| External Provider Integration Tests  | **14.6m** (lateral pole) | not gated on warm-sccache |
| Server Verification Harness          | **12.7m** | gated on warm-sccache (Path A pole) |
| Rust Clippy                          | 11.7m    | not gated |
| Engine Verification Harness          | 11.0m    | gated on warm-sccache |
| Warm sccache (leader)                | 10.2m    | gates the harness + coverage shards |
| Rust Runtime Tests                   | 9.9m     | not gated |
| Storage Verification Harness         | 8.3m     | gated on warm-sccache |
| Rust Dependency Audit                | 5.2m     | not gated |
| Coverage shards (max)                | 4.5m     | gated on warm-sccache |
| Coverage reducer                     | 3.8m     | gated on coverage shards |
| everything else                      | ≤ 1.5m   |   |

Workflow's critical path: `warm-sccache (10.2m) → Server Verification
Harness (12.7m)` = **22.9m**, very close to the 23.6m wall. The four
verification-harness jobs are sharded by surface but each surface is a
single un-sharded job. The two longest **lateral** jobs (Rust Workspace
Tests, External Provider Integration Tests) are unsharded monoliths.

If Server Verification Harness shards 3× to ~5m max, Path A drops to
~15.2m. If `Rust Workspace Tests` shards 3× to ~6m max and External
Provider Integration Tests splits per-provider to ~7m max, the lateral
poles also drop. The wall converges on `warm-sccache + 1 harness shard`
≈ 15m without infrastructure changes.

To go below 15m needs warm-sccache itself to shrink — that's the CW4
investigation lane.

## Scope

In scope:

- `.github/workflows/ci.yml` (verification-harness matrix expansion,
  workspace tests sharding, provider-test matrix split, warm-sccache
  optimization)
- `scripts/verification-harness.sh` (shard-arg surface; in-test corpus
  filter via `NIMBUS_HARNESS_SHARD`)
- `crates/nimbus-testing/src/harness/*` (corpus filter implementation
  honoring `NIMBUS_HARNESS_SHARD=N/M`)
- `scripts/verify-ci-wall-acceleration.sh` (this plan's verifier)
- `docs/operating/ci-modernization.md` (canonical contract update —
  CW1-CW4 sharding shapes + warm-sccache shape)
- Routing entries in `docs/plans/README.md` and `CLAUDE.md`

Out of scope:

- Caching mechanics already owned by archived CC plan
- CI infrastructure modernization (composite, SHA-pin, runner pin,
  job summaries, CodeQL) — owned by archived CM plan
- Coverage sharding / mold / release.yml composite — owned by
  archived CA plan
- New Rust workspace targets, test layouts, harness lanes
- Self-hosted runners / runner spec changes (different infra story)
- Signing / attestation / distribution — owned by
  `distribution-plan.md` family
- Windows release-build pole (CA5 deferred scope)

## Ledger

| CW  | Description | Status |
|-----|-------------|--------|
| CW0 | Scaffold this plan + the verifier at `scripts/verify-ci-wall-acceleration.sh` with the conditions enumerated in the Completion Gate. Routing entries added to `docs/plans/README.md` + `CLAUDE.md`. Baseline proof at `docs/plans/proof/ci-wall-acceleration/cw0-baseline.md` records per-job timings on `32951ee7` (the post-CA control commit) and the critical-path arithmetic. | done |
| CW1 | Shard `harness` matrix within each surface. Add `NIMBUS_HARNESS_SHARD=N/M` env-var support to the verification-harness corpus tests — each corpus reads the env var and filters cases by `case_index % M == (N-1)`. `scripts/verification-harness.sh` accepts a third arg `bash scripts/verification-harness.sh required server 1/4` and propagates the env var. CI matrix expands per-surface entries: `server` → 4 shards (7 transport-liveness cases dominate the duration; storage-history is 2 cases so cap is 2 but server inherits the higher count for the transport corpus), `engine` → 2 shards (corpus is 2 cases), `storage` → 1 (already 8.3m, below the wall pole), `runtime` → 1 (already 1.4m). Each shard runs independently; no reducer needed (the test passes iff every seed passes). Verifier asserts: harness script accepts `<shard>/<total>` form; matrix includes per-surface shard expansion. | done |
| CW2 | Shard `Rust Workspace Tests` via cargo-nextest `--partition hash:N/3`. The job's matrix expands to 3 shards. Doctests stay pinned to shard 1 (`make test-rust-docs` is ~30s and nextest does not support doctests so they cannot fan-out the same way). The Makefile target `test-rust-workspace` reads a `NIMBUS_NEXTEST_PARTITION` env var so the CI matrix sets the partition without leaking partition syntax into ci.yml itself. Verifier asserts: workspace-tests job sets the partition env var (or inlines `--partition`) AND has a matrix axis with ≥ 2 entries. | done |
| CW3 | Split `External Provider Integration Tests` by provider. Today the single job runs postgres + mysql + libsql serialized via `serial_test::serial(<provider>)`. Per-provider matrix expansion (`{provider=postgres,mysql,libsql}`) lets the three providers run in parallel; within-provider tests stay serial. `scripts/test-external-providers.sh` honors `NIMBUS_PROVIDER_FILTER=postgres|mysql|libsql` so each shard runs only its own provider's cargo invocations (postgres → storage + engine, same for mysql; libsql shard runs storage `libsql_provider` + engine `libsql_replica_provider`). The CI job drops the `services:` block and starts each fixture via `docker run` gated by `if: matrix.provider == ...`, including health-cmd loops for postgres/mysql parity. Verifier asserts: external-provider job has a `provider` matrix axis with ≥ 3 entries. | done |
| CW4 | Warm-sccache optimization (research lane). Two prongs: (a) drop `--tests` from the warm pass and let consumers pay test-binary compile inline (CI-minutes-neutral, may not move the wall); (b) prototype a per-target Swatinem cache layer for the warm-sccache job specifically, so its `target/` is restored between runs (not just `~/.cargo`). Land whichever holds up against the CW0 baseline; document the other as deferred scope. Verifier asserts: warm-sccache shape documented in `docs/operating/ci-modernization.md` and either (a) `--tests` dropped or (b) target-cache restored on the warm-sccache job. | pending |
| CW5 | Closeout. Flip every ledger row to `done`. Append Execution Log with real SHAs. Move plan to `docs/plans/archive/`. Promote `docs/operating/ci-modernization.md` with a new "PR critical-path acceleration" section synthesizing CW1-CW4 contracts. Update routing in `docs/plans/README.md` + `CLAUDE.md` to point at the archived path. Verifier's `plan_file()` helper accepts both active and archived paths. | pending |

## Completion Gate

`bash scripts/verify-ci-wall-acceleration.sh` exits 0 with summary
line `10 passed, 0 failed`. The 10 conditions:

1. Plan file exists (`docs/plans/ci-wall-acceleration-plan.md` or
   `docs/plans/archive/ci-wall-acceleration-plan.md`).
2. Routing entry exists in `CLAUDE.md` naming this plan.
3. Baseline proof exists at
   `docs/plans/proof/ci-wall-acceleration/cw0-baseline.md`.
4. `scripts/verification-harness.sh` accepts a third positional shard
   argument of the form `N/M` and the harness corpus test honors
   `NIMBUS_HARNESS_SHARD=N/M` (CW1).
5. The `harness` job matrix in `ci.yml` includes per-surface shard
   expansion (≥ 1 surface has `shard: [1/N, 2/N, ...]` entries with
   N ≥ 2; CW1).
6. The `rust-workspace-tests` job uses `nextest run ... --partition`
   and is a matrix with ≥ 2 shard entries (CW2).
7. The external-provider integration tests job has a `provider`
   matrix axis with ≥ 3 entries (CW3).
8. Warm-sccache lane is documented in `docs/operating/ci-modernization.md`
   ("PR critical-path acceleration" section) and either lands the
   `--tests` removal OR the target-cache restoration (CW4).
9. Every ledger row in this plan is marked `done`.
10. Latest CI run on main is green (`status=completed`,
    `conclusion=success`).

## Wall-time targets

- **CW0 baseline**: 23.6m wall on `32951ee7` (a doc-only commit).
- **Post-CW1**: Server Verification Harness shard max ~5m → Path A
  drops to ~15.2m. Engine/Storage harness shards drop in parallel.
- **Post-CW2**: Rust Workspace Tests max shard ~6m → lateral pole
  resolved.
- **Post-CW3**: External Provider Integration Tests max ~7m
  (postgres-dominated) → lateral pole resolved.
- **Post-CW4 best case**: warm-sccache 10.2m → 5-7m if (b) lands.
  Wall converges on ~12m.
- **Floor without infra changes**: ~12-15m, gated by `warm-sccache +
  1 server-harness shard` or `cold-build cost of the longest test
  binary`.

## Proof directory

`docs/plans/proof/ci-wall-acceleration/`:

- `cw0-baseline.md` — per-job CI timings on `32951ee7`; critical-path
  arithmetic; the CA delta vs the CA0 baseline (Coverage no longer
  the pole)
- `cw1-harness-sharding.md` — shard arg surface; in-test filter shape;
  matrix expansion diff; before/after Server Verification Harness
  timings
- `cw2-workspace-tests-sharding.md` — nextest --partition contract;
  matrix expansion diff; before/after timing
- `cw3-provider-matrix.md` — per-provider job split; serial_test
  contract within a provider; before/after timing
- `cw4-warm-sccache.md` — investigation summary; chosen lane;
  measured delta; deferred-scope notes
- `cw5-closeout.md` — final state, retro, total wall delta

## Execution Log

| CW  | Commit(s) | Subject |
|-----|-----------|---------|
| CW0 | `31e88d01` | scaffold CI Wall Acceleration plan + verifier + baseline proof |
| CW1 | `523834cb` | shard verification-harness corpus across N shards per surface |
| CW2 | `bacab0e1` | shard Rust Workspace Tests via nextest --partition |
| CW3 | `74e0ef8c` | split External Provider Integration Tests by provider |
| CW4 | _pending_ | warm-sccache optimization (selected lane) |
| CW5 | _pending_ | closeout — promote contract, archive plan, update routing |

## Notes on staging order

CW1 first because it hits the workflow's critical-path pole directly
(`warm-sccache → Server Verification Harness`). Each step is
independent — they could land in any order — but CW1 gives the largest
single-step wall delta (~7-8m off the critical path).

- **CW1 (harness shard)** changes the matrix shape in `ci.yml` and
  adds an in-test corpus filter. Per-surface shard counts are tuned
  to the surface's pre-CW timing: server gets 3 (12.7m → ~5m),
  engine gets 2 (11m → ~6m), storage gets 2 (8.3m → ~5m), runtime
  stays 1 (already 1.4m).
- **CW2 (workspace tests shard)** is a 1-line change to the cargo
  invocation plus matrix expansion. nextest's `--partition hash:N/M`
  hashes test paths so the partition is deterministic across runs.
- **CW3 (provider split)** is the simplest matrix expansion. The
  external-provider test crate already uses `serial_test::serial(<provider>)`,
  so within-provider serialization is preserved. Across providers
  was always safe — they connect to different services and don't
  share state.
- **CW4 (warm-sccache)** is research-first because the right lane
  isn't obvious from static inspection. CW4 may land lane (a) only,
  lane (b) only, both, or document both as deferred scope if the
  measured wall delta is < 30s.
- **CW5 (closeout)** is the contract-promotion step.

Within the wave, each CW is a separate commit so the Execution Log
SHAs are individually auditable.
