# Plan: CI Caching Canonicalization

Canonicalize the Rust + JS + Playwright caching architecture across all
nine `.github/workflows/*.yml` so a `push to main` (a) walks a single
shared content-addressed compilation cache instead of nine independent
3-4 GB monolithic `target/` slots, (b) deduplicates the UI build across
six Rust jobs via a single artifact, and (c) survives `gh run rerun`
without silently dropping the only good cache for the next push. Lands
sccache as the primary Rust compilation cache, uses Swatinem as a
secondary `target/` floor, fixes the rerun save-suppression bug,
introduces a build-ui artifact share, and re-tests the linker-parallelism
constraint that today forces `cargo-llvm-cov -j 1`.

---

## Status

- **Status:** `not started`
- **Created:** 2026-05-21
- **Primary owner:** this plan
- **Activation gate:** met. The current architecture has four
  load-bearing symptoms of one root issue (per-job monolithic
  `target/` caches with no cross-job sharing):

  1. **Coverage cache silently dropped on rerun.** Live cache state
     on 2026-05-21 shows 11 active caches totaling 32.49 GB on this
     repo; nine of them are 3-4 GB workspace caches (clippy,
     deny, runtime, harness ×4, external-providers, desktop-ui).
     The Coverage cache slot is **absent**. The previous green
     Coverage was a `gh run rerun --failed` on 44065b6c; the rerun
     hit a warm cache from the original failed run, made no
     observable change to `target/`, and Swatinem's
     "save-if-changed" heuristic skipped the save. Next push
     (4a958173) restored a 1-second "cache not found" and is cold-
     building the full instrumented workspace under a 45-min
     timeout — which is the only thing keeping it green.

  2. **27 GB of duplicated dep content.** The nine 3-4 GB Rust
     cache slots all store essentially the same workspace dep tree
     (rusty_v8, deno_core, tonic, sqlx, …) compiled identically
     under stable rustc. Per-rustc-call content-addressed caching
     (sccache) would dedupe this to ~3-5 GB total and remove the
     "one job's save displaces another's" contention.

  3. **Six Rust jobs each rebuild `nimbus-ui` from scratch.**
     `rust-workspace-tests`, `harness` ×4, `harness-nightly` ×4,
     `coverage`, and `rust-clippy` each run `npm ci` + `make
     build-ui` (~30-60 s each, ~3-5 min wall-clock waste per push).
     The UI dist is identical across all of them on a given commit;
     a single upstream `ui-artifacts` job + `actions/upload-artifact`
     + `actions/download-artifact` removes the duplication.

  4. **Workflow-level concurrency is unmodeled.** A push to main
     fires `ci.yml`, `desktop-ui.yml`, and `verify-nimbus-crun-patch.yml`
     simultaneously. They compete for the same cache pool with no
     coordination. A leader-job pattern (one short cold-cache warm-up
     job that other Rust jobs `needs:`) can convert a parallel
     cold-cold-cold start into a serial-cold-then-parallel-warm
     start; combined with sccache cross-job sharing this is the
     decisive win.

  The fix lands one sccache wiring + one Swatinem save-policy fix
  + one UI artifact share + one leader-job pattern + one
  `-j N` linker re-test. Each step is independently revertable.

## Why

### Architectural root cause: monolithic per-job `target/` caches

`Swatinem/rust-cache@v2` stores `target/` plus `~/.cargo/registry` plus
the `shared-key`-keyed metadata as a single tarball. Restoration is
all-or-nothing: any single-bit change in `Cargo.lock` or in the build
inputs invalidates the whole cache. Each Rust job in `ci.yml` defines
its own `shared-key`, producing nine independent caches that store
near-identical dep content.

This shape is right for repos with one or two Rust jobs. It is the
wrong shape for repos with eight-plus Rust jobs that all build the
same workspace. The canonical answer is content-addressed compilation
caching (`sccache`), which:

- Caches per-rustc-invocation by content hash (source + deps + flags +
  rustc version). One `serde_json` crate output is stored once and
  reused by every job that needs it.
- Composes naturally with `cargo`'s own incremental model
  (we already set `CARGO_INCREMENTAL=0`, which sccache requires).
- Survives Cargo.lock changes gracefully — only the changed crate
  (and its dependents) misses cache.
- Works across workflow boundaries (`ci.yml` populates the cache;
  `desktop-ui.yml` benefits from it).

### Survey: how comparable Rust repos solve this

| Project | Cache layer | Backend | Cross-job sharing | UI artifact share |
|---|---|---|---|---|
| **Rust** (rust-lang/rust) | sccache | S3 | Yes (every bootstrap shard) | N/A |
| **Servo** | sccache | S3 | Yes (every CI shard) | N/A |
| **Materialize** | sccache (`bin/ci-sccache`) | S3 | Yes (across all Rust jobs) | Buildkite artifacts |
| **Rspack** | sccache + Swatinem floor | GitHub Actions cache | Yes (Rust workers) | Pnpm artifact pool |
| **Tauri** | Swatinem only | GitHub Actions cache | No | N/A |
| **Meilisearch** | Swatinem only | GitHub Actions cache | No | N/A |
| **Tabby** | Swatinem only | GitHub Actions cache | No | N/A |

The empirical rule: repos with **2-3 Rust jobs** use Swatinem alone and
are happy. Repos with **8+ Rust jobs** that build a shared workspace
move to sccache + (optionally) Swatinem floor. We are in the second
category (9 Rust jobs in `ci.yml` alone, plus `desktop-ui.yml` and
`verify-nimbus-crun-patch.yml`'s helper compile work).

### Order-of-actions analysis (why sequencing matters)

**Multi-workflow concurrency on `push to main`:** `ci.yml` (9 Rust
jobs), `desktop-ui.yml` (1 Rust job), `verify-nimbus-crun-patch.yml`
(no Rust) all fire simultaneously. They share the runner concurrency
limit and the cache pool. Without coordination they cold-build
identical workspace dep trees in parallel.

**Inside `ci.yml`, zero `needs:` between Rust jobs.** All nine start
simultaneously, all attempt cache restore in parallel. With sccache
naively added, *every* job cold-misses sccache on the very first push
because none of them has yet populated it. Cross-job sharing only kicks
in on push N+1. To get the benefit on push N, we need a **leader job**
(short, single-crate `cargo check`) whose `needs:` is upstream of every
other Rust job. The leader populates sccache; followers run in parallel
with warm cache.

**Save-time clustering.** On a push, the 9 Rust caches save in a 7-min
burst (today's evidence: 14:34Z-14:41Z). Concurrent saves stress the
network and risk Coverage (the long pole) timing out *during save*.
Sccache stores small per-call entries asynchronously and avoids the
burst.

**Rerun save suppression.** Today's evidence is decisive: a `gh run
rerun --failed` on Coverage finished green but left no cache. Most
likely cause: Swatinem detected `target/` was unchanged from the
restored hot state and skipped the save. Either a) we set
`save-always: true` on Swatinem (forces save on every job, costs an
extra ~30s per job for re-upload), or b) sccache replaces Swatinem
entirely (per-rustc save semantics are inherently incremental and
don't have this failure mode), or c) we add an explicit "cache health
check" job that fails CI if expected cache slots are absent.

**Step ordering within Coverage:** today the order is `checkout →
toolchain → cache restore → npm setup → npm install → make build-ui →
cargo-llvm-cov`. The npm + UI steps add ~30-60 s even when cargo cache
is warm. With the build-ui artifact share, those steps drop to a
single ~5 s `download-artifact` call.

### Out of scope

- Replacing GitHub Actions with another CI provider (Buildkite, Cirrus,
  self-hosted) — separate, multi-quarter decision; not justified by
  current pain.
- Migrating to self-hosted runners on persistent disk — solves caching
  trivially but adds operational ownership we are not ready to take on
  pre-launch.
- Replacing cargo-llvm-cov with cargo-tarpaulin or grcov — coverage
  tool choice; orthogonal.
- Reorganizing the Rust workspace to reduce dep count — multi-month
  refactor with its own risk surface; not the right lever for CI
  wallclock.
- Switching the linker (mold vs rust-lld vs lld) for Coverage builds —
  considered but coverage instrumentation has known interactions with
  non-default linkers; CC6 re-tests the `-j N` constraint with the
  existing linker first.
- Cache observability dashboards beyond what `sccache --show-stats`
  emits (deferred — see "Successor work").

## Target architecture

After this plan, the canonical contract is:

```text
On every push:
  Workflow: ci.yml
    Job 1 (leader): warm-sccache
      runs-on: ubuntu-latest
      timeout-minutes: 15
      steps:
        - actions/checkout
        - dtolnay/rust-toolchain@stable
        - mozilla-actions/sccache-action
        - Swatinem/rust-cache (workspace floor)
        - cargo check --workspace --no-default-features --tests
        - sccache --show-stats (telemetry)
    Job 2 (leader): ui-artifacts
      runs-on: ubuntu-latest
      timeout-minutes: 10
      steps:
        - actions/checkout
        - actions/setup-node (cache: npm)
        - npm ci
        - make build-ui
        - actions/upload-artifact (packages/nimbus-ui/.nimbus + dist)

    Jobs 3..N (followers): clippy, runtime-tests, workspace-tests,
                            external-providers, harness ×4, coverage
      needs: [warm-sccache, ui-artifacts]
      steps:
        - actions/checkout
        - dtolnay/rust-toolchain@stable
        - mozilla-actions/sccache-action  (warm)
        - Swatinem/rust-cache             (warm floor)
        - actions/download-artifact (ui-artifacts)
        - cargo …                          (sccache hits ~80%)

Push N hit rate: ~80% (leader populated)
Push N+1 hit rate: ~95% (sccache cross-push)
```

## Ledger

| ID  | Phase                                                                                              | Status      |
|-----|----------------------------------------------------------------------------------------------------|-------------|
| CC0 | This plan written and committed under `docs/plans/ci-caching-canonicalization-plan.md`; indexed in `docs/plans/README.md` under "Active execution plans"; added a routing entry in `AGENTS.md` under "Routing By Work Type" for "CI caching / sccache / Swatinem orchestration"; baseline cache state and cold-cache Coverage timings captured as a proof under `docs/plans/proof/ci-caching-canonicalization/baseline-cache-state.json` and `baseline-coverage-timings.md`. The aggregate completion-gate verifier `scripts/verify-ci-caching-canonicalization.sh` (~150 LOC, modeled on `scripts/verify-local-dev-canonicalization.sh`) is checked in alongside the plan so the `/goal` control plane is verifiable from day one — at CC0 close it asserts every Completion-Gate condition and fails on the ones CC1-CC7 still need to satisfy. | done |
| CC1 | Add `mozilla-actions/sccache-action@v0.0.6` to the Coverage job only with `RUSTC_WRAPPER=sccache` and `SCCACHE_GHA_ENABLED=true`. Keep the existing Swatinem cache slot as a floor (no key change). Add a `Generate coverage report` post-step that prints `sccache --show-stats`. Push, observe 5-10 CI runs, verify (a) coverage build wallclock vs baseline, (b) sccache hit rate trends upward across runs, (c) no proc-macro or build.rs miscaching (test outputs unchanged), (d) Coverage cache slot now reliably present in `gh api repos/nimbus/nimbus/actions/caches` after every run. Capture proof at `docs/plans/proof/ci-caching-canonicalization/cc1-coverage-only-stats.md`. | not started |
| CC2 | Expand sccache uniformly to every Rust job: `rust-clippy`, `rust-runtime-tests`, `rust-workspace-tests`, `external-provider-tests`, `harness ×4`, `harness-nightly ×4`, `coverage` in `ci.yml`; `desktop-ui-smoke` in `desktop-ui.yml`; both jobs in `node-compat-nightly.yml`. Use a single shared sccache backend (GHA cache backend). Bump every Swatinem `shared-key` from `*-v1` to `*-v2` to deliberately rotate the floor caches (otherwise the v1 monolithic slots persist alongside sccache, wasting ~27 GB). Verify: cross-job hit rate >70% on push N+2, total cache pool size drops by >50%. | not started |
| CC3 | Fix Swatinem rerun save-suppression. Add `save-if: ${{ github.ref == 'refs/heads/main' }}` to every Swatinem invocation **and** `save-always: true` so reruns force a save regardless of target/ change detection. Re-test the failure mode: trigger a CI run, cancel a job mid-build, `gh run rerun --failed`, verify the post-rerun cache state matches expectations. Document the rerun semantics in `docs/operating/ci-caching.md` (created in CC5). | not started |
| CC4 | Introduce the `ui-artifacts` leader job in `ci.yml`. Runs `npm ci` + `make build-ui` once, uploads `packages/nimbus-ui/.nimbus/convex/*` and `packages/nimbus-ui/dist/*` via `actions/upload-artifact@v7`. Add `needs: [ui-artifacts]` to every downstream Rust job that today runs `make build-ui` (rust-workspace-tests, external-provider-tests, harness ×4, harness-nightly ×4, coverage, desktop-ui-smoke). Each follower job replaces its `npm ci` + `make build-ui` steps with a single `actions/download-artifact@v7` step targeting the ui-artifacts. The Rust-only jobs (rust-format, deny, rust-runtime-tests, rust-clippy if Node-free, proof-helpers) do not pick up the dependency. Verify: total wall-clock CI minutes per push drops by ~3-5 min; downstream jobs no longer have any `Set up Node.js`/`Install JS dependencies`/`Build nimbus-ui artifacts` steps. | not started |
| CC5 | Introduce the `warm-sccache` leader job in `ci.yml`. Runs a single-job sccache + Swatinem restore + `cargo check --workspace --tests --no-default-features` to populate the shared sccache. Add `needs: [warm-sccache]` to all downstream Rust jobs that consume Rust compilation. Document the full caching architecture in `docs/operating/ci-caching.md`: how sccache and Swatinem layer; what every job depends on; how to triage cache misses; how to force a fresh cache rotation; how `gh run rerun` interacts with cache saves. Verify: on a fresh first push (after a sccache cache rotation), the warm-sccache job populates the cache and downstream jobs hit ~80% sccache rate on the same push. | not started |
| CC6 | Re-test the `cargo-llvm-cov -j 1` constraint. Run Coverage with `-j 2`, `-j 4` on a side branch 5-10 times each. If no rust-lld bus errors recur across the runs, relax the constraint to `-j 2` (or higher) in `ci.yml`. Capture the test branch run IDs in `docs/plans/proof/ci-caching-canonicalization/cc6-link-parallelism.md`. If bus errors recur, keep `-j 1` and add a note linking the test runs to the existing comment in `ci.yml:680-682`. | not started |
| CC7 | Coverage scope optimization. Add `--no-doc-tests` to the cargo-llvm-cov invocation to skip doc-test instrumentation (which rebuilds crates as `--test` harnesses, doubling link work). Verify coverage line-count delta is <2% across all covered crates (acceptable signal loss for the wallclock savings). Capture the before/after lcov diff at `docs/plans/proof/ci-caching-canonicalization/cc7-no-doctests.md`. | not started |
| CC8 | Plan closeout. Flip every ledger row to `done`; append the Execution Log with the actual commit SHAs. Move this file to `docs/plans/archive/ci-caching-canonicalization-plan.md`. Update `docs/plans/README.md`: remove the active entry, add a paragraph under "Current Reference Baselines" naming the CC scope and closeout date. Update the routing entry in `AGENTS.md` to point at the archived path. Verify CI green on main. The verifier script (shipped in CC0) is amended in its plan-file regex to also accept the archived path. | not started |

## Completion Gate

All ledger rows must be `done`. The aggregate stop condition for the
/goal control plane is **`bash scripts/verify-ci-caching-canonicalization.sh`
exits 0** (CC0 ships the script; it fails-actionably at first and
becomes green only as CC1-CC7 land). Conditions:

1. **Plan checked in.** `test -f docs/plans/ci-caching-canonicalization-plan.md`
   *or* `test -f docs/plans/archive/ci-caching-canonicalization-plan.md`.
2. **sccache wired into every Rust job.** `grep -nE 'mozilla-actions/sccache-action' .github/workflows/*.yml` returns hits for `ci.yml`, `desktop-ui.yml`, and `node-compat-nightly.yml`; every Rust job that today defines a Swatinem cache also defines `RUSTC_WRAPPER: sccache`.
3. **Swatinem keys rotated to v2.** `grep -nE 'shared-key:.*-no-bin-v1' .github/workflows/*.yml` returns zero matches; `grep -nE 'shared-key:.*-no-bin-v2' .github/workflows/*.yml` returns the expected nine matches.
4. **Rerun-safe save policy.** Every Swatinem invocation has `save-always: true`.
5. **`ui-artifacts` job exists and is consumed.** `grep -nE '^  ui-artifacts:' .github/workflows/ci.yml` returns a match; every Rust job that previously ran `make build-ui` now declares `needs: [ui-artifacts]` and uses `actions/download-artifact@v7` targeting `ui-artifacts`.
6. **`warm-sccache` job exists and is consumed.** `grep -nE '^  warm-sccache:' .github/workflows/ci.yml` returns a match; downstream Rust jobs declare `needs: [warm-sccache]`.
7. **Caching contract documented.** `test -f docs/operating/ci-caching.md`.
8. **Routing entry exists.** `grep -nE 'ci-caching-canonicalization-plan' CLAUDE.md` returns at least one line.
9. **All proof captures present.** `test -f docs/plans/proof/ci-caching-canonicalization/baseline-cache-state.json` AND `test -f docs/plans/proof/ci-caching-canonicalization/cc1-coverage-only-stats.md` AND `test -f docs/plans/proof/ci-caching-canonicalization/cc6-link-parallelism.md` AND `test -f docs/plans/proof/ci-caching-canonicalization/cc7-no-doctests.md`.
10. **Ledger rows all done.** Every row in the ledger ends with `| done |`.
11. **Branch state.** `git log --oneline origin/main..HEAD` is empty.
12. **CI green on main.** `gh run list --branch main --workflow=CI --limit 1 --json conclusion -q '.[0].conclusion'` returns `success`.

## Risks

- **R1 — sccache miscaching of build.rs outputs.** sccache hashes
  rustc invocations; build.rs is compiled and cached. If a build.rs
  reads non-deterministic input (file mtimes, env vars), the cache
  becomes stale. Today `crates/nimbus-server/build.rs` (post-LD2) only
  reads `cargo:rerun-if-changed` deps; no network or wall-clock
  input. Tonic-build invokes protoc but is content-addressed on
  proto files. Both should be safe. Mitigation: CC1 verifies test
  outputs match across cached/uncached runs before expanding to
  other jobs.
- **R2 — sccache GHA cache backend ceiling.** GitHub Actions cache
  is repo-scoped with a soft size budget. sccache adds many small
  entries (~1 per rustc call). If we exceed the budget, the cache
  service evicts oldest entries, degrading hit rate. Mitigation:
  monitor `gh api repos/.../actions/caches` per-push; CC5 captures
  steady-state size. If we exceed budget, move to S3 backend
  (separate plan, but the wiring is one env-var change).
- **R3 — Leader job adds wall-clock for first push after rotation.**
  After CC2's v1→v2 rotation, the first push has no sccache to
  restore. The leader job builds the workspace from scratch (~15 min).
  Downstream jobs start later but with warm sccache. Net wall-clock
  is similar to today on a one-shot basis; the win is amortization
  across pushes. Mitigation: CC2 schedules the v1→v2 rotation
  immediately before a low-traffic window; CC5 documents.
- **R4 — Concurrent Swatinem + sccache double caching.** During the
  transition (CC1 ships sccache to Coverage only, CC2 expands), both
  layers coexist. They consume cache budget independently. Mitigation:
  CC2 rotates Swatinem v1→v2 simultaneously with sccache expansion,
  forcing old monolithic caches to age out.
- **R5 — `harness` matrix surfaces use different feature flags.**
  Each surface (storage/engine/server/runtime) may exercise different
  cargo features, which affect sccache cache keys. If hit rate is
  low across the matrix, the per-surface caching benefit is muted
  but the within-surface benefit holds. Mitigation: measure per-
  surface hit rate in CC2.
- **R6 — `-j N` re-test in CC6 may show bus errors only intermittently.**
  rust-lld parallelism bugs are notoriously hard to reproduce. CC6's
  5-10 runs may not be statistically conclusive. Mitigation: if any
  run shows a bus error, treat as "constraint still applies" and
  keep `-j 1`; document the run IDs.
- **R7 — `actions/upload-artifact` and `download-artifact` have a
  100 GB limit and 90-day retention.** UI artifacts are small (~10
  MB) so neither limit is at risk. No mitigation needed.
- **R8 — UI artifact share serializes downstream jobs behind
  ui-artifacts.** ~3-5 min added to the critical path of any job
  that previously parallel-built the UI. Net win because ui-artifacts
  finishes in ~3-4 min and the 6 downstream jobs save ~30-60 s each.
  Mitigation: measure in CC4.

## Control plane (/goal invocation)

`scripts/verify-ci-caching-canonicalization.sh` ships in CC0, so the
/goal stop condition is verifiable from day one. The canonical
invocation that drives this plan to completion autonomously is:

```
/goal bash scripts/verify-ci-caching-canonicalization.sh exits 0
```

The stop hook re-evaluates that command on every prompt; once the
script exits 0, the goal is satisfied and the loop terminates.

During execution the autonomous loop should:

1. Read this plan and pick the next `not started` ledger row.
2. Make the smallest change that moves that row to `done`.
3. Commit with a focused message that names the CC id.
4. Push to main (no PR — pre-launch; this plan inherits the same
   autonomous-mode authorization scope as the LD plan family by
   user request).
5. Update this plan: flip the row from `not started` → `done`,
   commit the plan update.
6. Repeat until CC8 closes the plan.

The order CC0 → CC1 → CC2 → CC3 → CC4 → CC5 → CC6 → CC7 → CC8 is the
recommended sequence. CC1 must precede CC2 (validate sccache on one
job before expanding). CC3 (rerun-safety) can ship any time after CC2
but should precede CC5 (which documents the policy). CC4
(ui-artifacts) and CC6 (`-j N`) are independent of the sccache chain
and can swap in. CC7 (`--no-doctests`) requires CC1 or later (so we
can measure the Coverage delta with sccache in place). CC8 closes
the plan.

## Successor work (deferred, separate plans)

- **S3 sccache backend.** If the GHA cache backend hit rate is poor
  (<70%) or budget pressure becomes a recurring issue, migrate
  sccache to S3. Requires AWS account, IAM, secret wiring. One
  ledger row, scoped to the backend swap. Activation gate:
  >2 weeks of GHA-backend hit-rate telemetry showing the issue.
- **Self-hosted runners with persistent disk.** Replaces both
  Swatinem and sccache entirely. Operationally heavier; not
  justified pre-launch. Activation gate: CI wallclock is a
  recurring complaint and we have ops headcount.
- **Coverage matrix split.** If Coverage remains the long pole
  after CC6 + CC7, split it into 2-3 jobs by `--package` group.
  More parallelism, more complexity. Activation gate: Coverage
  wallclock >25 min after CC6+CC7 lands.
- **Cross-OS sccache sharing in `release.yml`.** Windows / macOS /
  aarch64 release builds also use Swatinem. sccache supports
  cross-OS but requires careful key design. Activation gate:
  release.yml wallclock becomes a recurring pain.
- **Cache observability dashboards.** A nightly workflow that
  reads `gh api .../actions/caches`, plots size + hit rate per
  shared-key, and posts to a tracked dashboard. Activation gate:
  recurring cache surprises after this plan.

## Execution Log

Will be appended as each CC lands on main.

| CC  | Commit(s) | Subject |
|-----|-----------|---------|
| CC0 | _pending_ | _pending_ |
| CC1 | _pending_ | _pending_ |
| CC2 | _pending_ | _pending_ |
| CC3 | _pending_ | _pending_ |
| CC4 | _pending_ | _pending_ |
| CC5 | _pending_ | _pending_ |
| CC6 | _pending_ | _pending_ |
| CC7 | _pending_ | _pending_ |
| CC8 | _pending_ | _pending_ |
