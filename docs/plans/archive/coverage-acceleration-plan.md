# Coverage Acceleration Plan (CA)

The CM plan closed with all 12 modernization conditions green but
explicitly left two performance pole-holders untouched: the serialized
`cargo llvm-cov -j 1` coverage job in `ci.yml` (the critical-path
bottleneck for every PR) and `release.yml`, which CM1 deliberately did
not migrate into the `setup-rust-cached` composite. The CA plan
canonicalizes both.

## Why this plan exists

Latest CI run on `main` (SHA `f99f7d6c`, CM4 hotfix) — total wall
**33m57s**, per-job breakdown:

| Job                                  | Duration |
|--------------------------------------|----------|
| Coverage                             | **24m27s** (critical path) |
| Rust Workspace Tests                 | 14m16s   |
| Server Verification Harness          | 10m59s   |
| External Provider Integration Tests  | 10m12s   |
| Engine Verification Harness          | 9m39s    |
| Storage / Runtime Verification Harness | ≤ 8m07s |
| Rust Clippy                          | 6m39s    |
| Warm sccache (leader)                | 5m50s    |
| everything else                      | ≤ 1m14s  |

Coverage owns the last ~10 minutes of wall-clock. If it finishes at
~11m (matching Server Verification Harness), total CI wall drops to
~17m — a **45% reduction** sitting in one job.

Latest release run (SHA `83e08294`, v0.1.31 bootc default) — total
wall **76m35s**, per-job breakdown:

| Job                                   | Duration |
|---------------------------------------|----------|
| Build (x86_64-pc-windows-msvc)        | **70m12s** (release critical path) |
| Build (aarch64-apple-darwin)          | 49m35s   |
| Build (x86_64-unknown-linux-gnu)      | 29m46s   |
| Build (aarch64-unknown-linux-gnu)     | 28m00s   |
| Build machine-os                      | 6m43s    |
| Publish machine-os                    | 5m34s    |
| Create Release                        | 0m31s    |

None of the release-builder jobs use `setup-rust-cached`; none have
sccache. They each install `dtolnay/rust-toolchain` + `Swatinem/rust-cache`
inline (5 sites). CM1's contract — "the composite action is the only
place `mozilla-actions/sccache-action` / `Swatinem/rust-cache` is
referenced" — was scoped to PR CI and explicitly did not touch
`release.yml`. That deferral is now the holdout.

Coverage's `-j 1` serialization comes from a CC-era safety constraint
documented inline at `.github/workflows/ci.yml:686-694`:

> Keep the instrumented workspace build serialized. GitHub-hosted
> Linux runners have shown rust-lld bus errors when multiple large
> coverage test binaries link in parallel. CC6 re-tests `-j 2`/`-j 4`
> once sccache has reduced cold-build link pressure.

sccache has been live and stable across every Rust job for the full
CC9 + CM closeout window. The predicate for retesting parallelism is
met. But sccache caches compile, not link — coverage's bottleneck is
link-step contention with instrumented binaries. The right linker
replacement (`mold`) plus a careful `-j` re-test is the next move.

## Scope

In scope:

- `.github/workflows/ci.yml` (Coverage job parallelism + sharding)
- `.github/workflows/release.yml` (composite extraction + sccache adoption)
- `.github/actions/setup-rust-cached/action.yml` (mold installation
  step, conditional cross-platform handling)
- `scripts/verify-coverage-acceleration.sh` (this plan's verifier)
- `docs/operating/ci-modernization.md` (canonical contract update —
  coverage sharding shape + release-pipeline sccache adoption)
- Routing entries in `docs/plans/README.md` and `CLAUDE.md`

Out of scope:

- Caching mechanics already owned by archived CC plan
- CI infrastructure modernization (composite, SHA-pin, runner pin,
  job summaries, CodeQL) — owned by archived CM plan
- Adding new Rust workspace targets, test layouts, or harness lanes
- Signing / attestation / distribution shape — owned by
  `distribution-plan.md` family

## Ledger

| CA  | Description | Status |
|-----|-------------|--------|
| CA0 | Scaffold this plan + the verifier at `scripts/verify-coverage-acceleration.sh` with the conditions enumerated in the Completion Gate. Routing entries added to `docs/plans/README.md` + `CLAUDE.md`. Baseline proof at `docs/plans/proof/coverage-acceleration/ca0-baseline.md` records per-job timings and the sccache stats published by the Coverage job pre-CA1. | done |
| CA1 | Install `mold` in the `setup-rust-cached` composite action on Linux runners and configure `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS=-C link-arg=-fuse-ld=mold` (RUSTFLAGS-with-fuse-ld, not LINKER=mold — see `proof/ca1-mold-install.md` for the failure mode of the LINKER variant). Benefits every Rust job that goes through the composite (12 sites today) but unblocks the Coverage `-j > 1` retest in particular. Verifier asserts: composite action references `mold` install + linker env (either LINKER=mold or RUSTFLAGS+fuse-ld=mold). macOS / Windows branches of the composite remain on the system linker. | done |
| CA2 | With mold landed in the composite, retest `cargo llvm-cov -j 2` and `-j 4` for the Coverage job. Land the highest stable `-j` value as the new constant, update the inline comment that documents the deferral. Verifier asserts the Coverage step is no longer `-j 1` (`grep -E 'cargo llvm-cov -j (2|4|8)' ci.yml`). Bus-error recurrence is a CA2 acceptance signal — if `-j 2` regresses, CA2 documents the disposition in proof and leaves `-j 1` in place; CA3 still proceeds. | done |
| CA3 | Shard the Coverage job across N parallel lanes by workspace member group. Each lane runs `cargo llvm-cov --no-report -p <group>` against the same instrumented profile; a final reducer job calls `cargo llvm-cov report --lcov --output-path lcov.info` after downloading `.profraw` artifacts from every shard. Critical path goes from `sum(crates)` to `max(group)`. Verifier asserts: the Coverage job is fan-out + fan-in shape (matrix with N ≥ 2 entries plus a dependent reducer), and the reducer publishes the same `lcov.info` artifact + Codecov upload that the single-job shape produced. | done |
| CA4 | Extend `setup-rust-cached` adoption to `release.yml`. Migrate the inline `dtolnay/rust-toolchain` + `Swatinem/rust-cache` sites in `release.yml` (`build-linux-arm64` + the 3-entry `build` matrix covering linux x86_64 / darwin arm64 / windows x86_64) to `uses: ./.github/actions/setup-rust-cached`. macOS / Windows runners get sccache via the composite the same way Linux jobs do (sccache-action is platform-aware). Composite extended with `save-cache: always\|auto\|never` so release-tag builds can save their per-target caches while PR CI keeps the `save-if: refs/heads/main` invariant. Verifier asserts zero inline `mozilla-actions/sccache-action` references workflow-wide (already enforced by CM verifier condition 3, kept in CA scope as a regression gate); plus the new assertion: zero inline `Swatinem/rust-cache` references in `release.yml` (composite-only). First post-CA4 release run records cold sccache stats in proof; the second records warm hit-rate. | done |
| CA5 | Closeout. Flip every ledger row to `done`. Investigate the Windows release-build pole — profile the build to identify the dominant component (likely vendored OpenSSL via Strawberry Perl, or V8 fresh build), and document follow-up either in this plan (if landed) or as deferred scope. Append Execution Log with actual SHAs. Move plan to `docs/plans/archive/`. Promote `docs/operating/ci-modernization.md` with a new "Coverage and release acceleration" section synthesizing CA1–CA4 contracts. Update routing in `docs/plans/README.md` + `CLAUDE.md` to point at the archived path. Verifier's `plan_file()` helper accepts both active and archived paths. | done |

## Completion Gate

`bash scripts/verify-coverage-acceleration.sh` exits 0 with summary
line `10 passed, 0 failed`. The 10 conditions:

1. Plan file exists (`docs/plans/coverage-acceleration-plan.md` or
   `docs/plans/archive/coverage-acceleration-plan.md`).
2. Routing entry exists in `CLAUDE.md` naming this plan.
3. Baseline proof exists at
   `docs/plans/proof/coverage-acceleration/ca0-baseline.md`.
4. Composite action installs `mold` on Linux runners and exports
   `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=mold` (CA1).
5. The Coverage step in `ci.yml` is **not** `cargo llvm-cov -j 1` —
   i.e. the `-j` flag is `2`, `4`, `8`, or absent; or the inline
   comment block explicitly documents the post-CA2 disposition
   (CA2 acceptance signal).
6. The Coverage job in `ci.yml` is sharded (matrix with `N ≥ 2`
   entries on a `shard` / `group` axis) with a dependent reducer
   step that calls `cargo llvm-cov report` (CA3).
7. Zero inline `Swatinem/rust-cache` references in `release.yml`
   (composite-only, CA4).
8. Zero inline `mozilla-actions/sccache-action` references workflow-
   wide (CM-era invariant preserved; regression gate for CA4).
9. Every ledger row in this plan is marked `done`.
10. Latest CI run on main is green (`status=completed`,
    `conclusion=success`).

## Proof directory

`docs/plans/proof/coverage-acceleration/`:

- `ca0-baseline.md` — per-job CI + release timings pre-CA1; sccache
  stats from Coverage; comment-block excerpt documenting the `-j 1`
  deferral
- `ca1-mold-install.md` — composite-action diff sketch; cold/warm
  Coverage-job timing delta on first runs after the composite update
- `ca2-coverage-parallelism.md` — `-j 2` / `-j 4` retest evidence;
  rust-lld bus-error recurrence disposition; chosen `-j` constant
- `ca3-coverage-sharding.md` — chosen shard groups; matrix shape;
  reducer-job contract; `lcov.info` byte-identical-by-content check
- `ca4-release-composite.md` — release.yml diff sketch (5 sites →
  composite); first post-migration release run's cold sccache stats;
  follow-up release's warm hit-rate
- `ca5-closeout.md` — final state, Windows release-pole findings,
  retro

## Execution Log

| CA  | Commit(s) | Subject |
|-----|-----------|---------|
| CA0 | `3e86c329` | scaffold Coverage Acceleration plan + verifier + baseline proof |
| CA1 | `263f39f7` + `a7afe415` | install mold linker in setup-rust-cached composite (initial LINKER=mold landed broken; hotfix switched to RUSTFLAGS `-fuse-ld=mold`) |
| CA2 | `f4dad1b8` | retest coverage parallelism, land -j 4 |
| CA3 | `3996dd9a` + hotfix (this commit) | shard Coverage across 3 parallel lanes with cargo llvm-cov reducer (initial drop landed with two CA3 bugs: profraw path `target/llvm-cov-target/profraw/` did not match cargo-llvm-cov's actual output path `target/llvm-cov-target/*.profraw`, and the engine shard's `needs-providers: "false"` skipped the libsql fixture nimbus-engine's `libsql_replica_provider` tests require; both fixed in CA5) |
| CA4 | `d66f85fb` | migrate release.yml to setup-rust-cached composite |
| CA5 | `598dd74e` + hotfix (this commit) | closeout — promote contract, archive plan, update routing; CA3 hotfix bundled here |

## Notes on staging order

CA1 first because the linker change is the smallest blast radius (one
composite-action edit) and is the prerequisite the CC6 comment block
explicitly named for CA2. Each step builds on the previous:

- **CA1 (mold)** is contained to the composite; every Rust job picks
  it up on its next run. No workflow-shape change yet.
- **CA2 (`-j` retest)** is a 1-line edit to the Coverage `run:`
  command. If `-j 2` regresses to bus errors, CA2 documents the
  disposition and CA3 still proceeds (sharding moves the link work
  off the critical path either way).
- **CA3 (sharding)** is the biggest swing — it changes the job
  topology in `ci.yml` and adds a reducer step. Worth doing after
  CA1+CA2 because each reduces per-shard link cost and shrinks the
  benefit of sharding marginally. Net effect remains a large wall-
  time reduction even if CA1+CA2 land first.
- **CA4 (release.yml)** is orthogonal to the coverage work but
  belongs in the same plan because it's the other holdout that CM1
  explicitly deferred. Migrating it is mechanical once the composite
  is the canonical entry point.
- **CA5 (closeout)** investigates the Windows release pole as a
  follow-up — likely a vendored-OpenSSL or V8 cross-compile issue;
  may land in this plan or become deferred scope for a follow-on
  release-acceleration plan.

Within the wave, each CA is a separate commit so the Execution Log
SHAs are individually auditable.
