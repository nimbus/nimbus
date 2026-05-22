# CI Caching Contract

This document is the canonical reference for how CI caches Rust compilation
across the `nimbus/nimbus` workflows. It explains the layering of sccache
and Swatinem/rust-cache, the leader jobs that produce shared artifacts,
how to triage cache misses, how to force a fresh rotation, and how
`gh run rerun` interacts with cache saves.

If you're staring at a CI run that took 22 minutes when you expected 8,
or you don't know which cache slot a job is consuming, you are in the
right place.

## The two cache layers

CI uses two complementary caches:

| Layer | Action | Granularity | Backend | Lifetime |
|-------|--------|-------------|---------|----------|
| Floor | `Swatinem/rust-cache@v2` | per-job (one slot per `shared-key`) | GitHub Actions cache | 7 days idle, ~10 GB per slot |
| Hot | `mozilla-actions/sccache-action@v0.0.6` (`RUSTC_WRAPPER=sccache`) | per-rustc-call (content-addressed) | GitHub Actions cache (shared across all jobs in repo) | 7 days idle, one shared pool |

**Swatinem** stores the `target/` directory plus `~/.cargo/registry` and
`~/.cargo/git` under a job-specific `shared-key`. On a hit it restores
the entire monolithic state, so subsequent `cargo build` runs barely
do any rustc work at all.

**sccache** sits as the `RUSTC_WRAPPER`, so every individual rustc
invocation looks up the input crate's compile inputs (source + Cargo
features + rustc flags) in a content-addressed store keyed by their
hash. Hits stream the prebuilt `.rmeta` / `.rlib` straight from the
cache; misses run rustc and write the result back. The pool is shared
across every job in the repo, so a rustc invocation in `harness` can
hit a result produced moments earlier by `coverage`.

The two layers stack: Swatinem is the floor that makes a single job
warm-start; sccache is the hot layer that lets parallel jobs reuse each
other's work.

## What runs sccache

Every job that invokes `rustc` (directly or transitively via `cargo`)
has `mozilla-actions/sccache-action@v0.0.6` installed and uses
`RUSTC_WRAPPER=sccache`. The env vars are set at workflow level in:

- `.github/workflows/ci.yml`
- `.github/workflows/desktop-ui.yml`
- `.github/workflows/node-compat-nightly.yml`

Jobs that only run `cargo fmt`, `cargo deny`, or `npm`/bash commands
inherit the env vars but never invoke rustc, so the wrapper is a no-op
there.

## Leader jobs

`ci.yml` has two leader jobs that produce shared artifacts the rest of
the workflow consumes.

### `ui-artifacts`

Runs `npm ci` + `make build-ui` once per push and uploads
`packages/nimbus-ui/.nimbus/convex` (convex codegen output) and
`packages/nimbus-ui/dist` (SPA build) via `actions/upload-artifact@v7`
under the name `ui-artifacts` with a 1-day retention.

Consumers (`needs: [ui-artifacts]` or downstream of it):

- `warm-sccache` (compiles nimbus-server, which `include_str!`s the
  convex outputs and `rust-embed`s the SPA dist)
- `harness` (matrix, all 4 surfaces)
- `coverage`

Each consumer replaces what used to be `Set up Node.js` +
`Install JS dependencies` + `Build nimbus-ui artifacts` with
`actions/download-artifact@v7` + a `find … -exec touch +` step that
refreshes mtimes so Make's `UI_DIST_INDEX` target stays satisfied and
no transitive rebuild kicks in.

### `warm-sccache`

Runs `cargo check --workspace --tests` once with sccache wired, so
every workspace crate (and its dev-deps) gets rustc'd into the shared
sccache pool. Downstream parallel Rust jobs then hit on the same push
instead of cold-compiling identical deps in parallel.

Consumers (`needs: [warm-sccache]`):

- `harness` (matrix, all 4 surfaces)
- `coverage`

`warm-sccache` itself `needs: [ui-artifacts]` because it compiles
nimbus-server, so the chain is
`ui-artifacts → warm-sccache → harness / coverage`.

## Job dependency graph

```
ui-artifacts ──┬──> warm-sccache ──┬──> harness (× 4 surfaces)
               │                   └──> coverage
               └─> (other Rust jobs run without warming)
```

Jobs that **don't** need a leader (they cold-start and rely only on
their own Swatinem slot + sccache pool):

- `rust-format` — runs `cargo fmt`, no rustc
- `rust-clippy` — runs `make clippy` with its own UI build path
- `deny` — `cargo deny check`, no rustc compile
- `rust-runtime-tests` — `make test-rust-runtime` (nimbus-runtime has
  zero workspace deps, so no UI required)
- `rust-workspace-tests` — `make test-rust-workspace` (still builds UI
  transitively via Make)
- `external-provider-tests` — `make test-external-providers`

These are intentional carve-outs: they either don't compile much, or
they're independent enough that paying a serial wait on a leader
exceeds the savings from a warmer sccache.

## Swatinem shared-keys

Every Swatinem invocation uses a unique `shared-key` so each job's
floor cache is independent. The current keys (all bumped to `-v2`
in CC2) are:

| Job | shared-key |
|-----|-------------|
| `rust-clippy` | `ci-ubuntu-stable-clippy-no-bin-v2` |
| `deny` | `ci-ubuntu-stable-deny-no-bin-v2` |
| `rust-runtime-tests` | `ci-ubuntu-stable-runtime-no-bin-v2` |
| `rust-workspace-tests` | `ci-ubuntu-stable-workspace-no-bin-v2` |
| `external-provider-tests` | `ci-ubuntu-stable-external-providers-no-bin-v2` |
| `warm-sccache` | `ci-ubuntu-stable-warm-sccache-no-bin-v2` |
| `harness` matrix | `ci-ubuntu-stable-harness-${{ matrix.surface }}-no-bin-v2` |
| `harness-nightly` matrix | `ci-ubuntu-stable-harness-nightly-${{ matrix.surface }}-no-bin-v2` |
| `coverage` | `ci-ubuntu-stable-coverage-no-bin-v2` |
| `desktop-ui-smoke` | `ci-ubuntu-stable-desktop-ui-v2` |
| `node-compat-rust-corpus` | `node-compat-rust-corpus-ubuntu-stable-no-bin-v2` |
| `node-compat-evidence` | `node-compat-nightly-ubuntu-stable-no-bin-v2` |

All invocations set:

```yaml
cache-on-failure: "true"
cache-bin: "false"
save-if: ${{ github.ref == 'refs/heads/main' }}
save-always: true
```

`save-if` restricts cache writes to pushes on `main` (PR builds restore
but don't save, so PR builds can't pollute the shared slot). The
`cache-bin: "false"` keeps `target/release` binaries out of the cache
(they bloat the slot and aren't useful for next-build deltas).
`save-always: true` forces a save on reruns even when Swatinem's
"save-if-changed" heuristic would otherwise skip it.

## How to read sccache stats

Every job that runs sccache prints `sccache --show-stats` as an
`always()` step. The output looks like:

```
Compile requests           623
Compile requests executed  623
Cache hits                 511 (82.0%)
Cache misses               112
Cache hits rate            82.0%
```

Healthy steady-state target: **>70% cache hit rate on warm jobs**
(after `warm-sccache` runs), **>80% on the second push of the same
session** (sccache populated, Swatinem warm).

A sudden drop in hit rate after a dep upgrade is expected (sccache
keys include rustc flags and dep tree; new dep versions miss). One or
two pushes restore the rate.

## How to triage cache misses

1. **Find the failing job's sccache step output**. It dumps hits,
   misses, and storage backend.
2. **Compare hit rate to last successful push** on the same branch.
3. **If both jobs miss the same crate**: it's a content-key
   change — recent edit to that crate, a dep version bump, or a
   rustc-flag tweak. Expected.
4. **If only one job misses**: feature-flag delta. Different jobs
   compile crates with different `--features`; sccache keys the
   feature set, so this is correct, not a regression.
5. **If sccache stats show storage errors**: the GitHub Actions cache
   backend is unavailable. Re-run the job; transient.
6. **If `Cache cargo artifacts` reports "cache not found"**: Swatinem
   slot expired (7-day idle) or was rotated (see below). The next
   `save-always` save will repopulate it.

## How to force a fresh cache rotation

Two reasons to rotate:

1. **Stale target/ poisoning** — incremental builds wedged on
   inconsistent state. Symptom: rustc internal compiler errors,
   "phantom" type errors on `cargo check` of a clean tree.
2. **Cache pool bloat** — `gh api repos/nimbus/nimbus/actions/caches`
   shows >40 GB across slots, evicting useful entries.

To rotate, bump every Swatinem `shared-key` suffix (e.g., `-v2` →
`-v3`) in:

- `.github/workflows/ci.yml` (8 invocations)
- `.github/workflows/desktop-ui.yml` (1 invocation)
- `.github/workflows/node-compat-nightly.yml` (2 invocations)

The next push then writes fresh slots; the old `-v2` slots age out
under the 7-day idle policy.

To rotate **only** the sccache pool (rare): change `SCCACHE_GHA_ENABLED`
to `false` for one push (forces miss-everything) and back. The
`mozilla-actions/sccache-action@v0.0.6` keys by repo+ref+OS, so an
explicit cache eviction via `gh cache delete` is the simpler tool.

## How `gh run rerun` interacts with cache saves

Before CC3, reruns of a partially-failed job would suppress the
Swatinem save: the rerun re-restored the same cache that existed
before the first run, did the work, and Swatinem's
"save-if-target/-changed" heuristic decided nothing changed and
skipped the save. The next push then cold-rebuilt everything because
the slot had aged or been evicted.

CC3 added `save-always: true` to every Swatinem invocation, so
reruns now save unconditionally on `main`. Combined with
`save-if: refs/heads/main`, the contract is:

- **Push to main**: every Swatinem invocation saves on success or
  failure (`cache-on-failure: true`) **and** on rerun
  (`save-always: true`).
- **PR push**: every Swatinem invocation restores but does not save.
- **Schedule (`harness-nightly`)**: saves on `main` (which is where
  the schedule runs).

To verify the rerun-save behavior:

```bash
# 1. Push a CI run on main.
git push origin main
RUN_ID=$(gh run list --branch main --workflow=CI --limit 1 --json databaseId -q '.[0].databaseId')

# 2. Cancel one job mid-build.
gh run cancel "${RUN_ID}"

# 3. Rerun failed jobs.
gh run rerun "${RUN_ID}" --failed

# 4. Verify the cache slot was saved.
gh api repos/nimbus/nimbus/actions/caches | jq '.actions_caches[] | select(.key | contains("ci-ubuntu-stable-coverage"))'
```

The expected output is a fresh `created_at` timestamp on the matching
`-v2` slot after the rerun completes.

## Concurrency context

A push to `main` fires three workflows simultaneously:

- `ci.yml` (9 Rust jobs + 2 leader jobs)
- `desktop-ui.yml` (1 Rust job)
- `verify-nimbus-crun-patch.yml` (no Rust compilation)

All Rust jobs across all three workflows share the **same sccache
pool**. So a rustc invocation in `desktop-ui-smoke` can hit a result
produced by `harness` moments earlier in the same push window. That's
why the sccache layer is workflow-wide, not per-workflow.

## Sources

- The active execution plan is `docs/plans/ci-caching-canonicalization-plan.md`
  (will be archived under `docs/plans/archive/` after CC8).
- Aggregate verifier: `scripts/verify-ci-caching-canonicalization.sh`.
- Baseline timings: `docs/plans/proof/ci-caching-canonicalization/baseline-coverage-timings.md`.
- Baseline cache state: `docs/plans/proof/ci-caching-canonicalization/baseline-cache-state.json`.
