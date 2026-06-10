# PW4c — warm-sccache retained

## Decision

Keep `warm-sccache:` in `ci.yml`. Do not retire (PW4b path skipped).

PW success target reverts to the PW4c wall threshold: PR wall ≤ 18m
(the verifier's condition 8 flips from 15m → 18m when warm-sccache
remains present, per `scripts/verify-ci-pr-wall-sub-15.sh:246-250`).

## Why measurement could not justify retirement

PW4a's plan was to sample 7 days of post-PW3 PR runs and aggregate
Swatinem cache hit rate per job. Two conditions prevent that:

1. **PW3 just landed.** The very first post-PW3 main run
   (`81999b83`, the PW3 SHA-backfill commit) is still in flight at
   the time PW4 is evaluated. There is zero post-PW3 baseline data.
2. **Pre-PW3 data is contaminated.** Before PW3, `ci.yml` used
   unconditional `cancel-in-progress: true`. Back-to-back main
   pushes (the normal PW execution pattern: each PW lands a change
   commit + a SHA-backfill commit seconds apart) cancelled the
   first run mid-flight. A cancelled run does not run its
   cache-save post-step, so Swatinem and sccache state-after
   diverge from state-before. The "hit rate" computed against
   pre-PW3 logs would be measuring the bug PW3 fixed, not the
   steady-state behaviour.

## What the available CW5 data does show

Sampled CW5 (`23eb430e`, last successful main run pre-PW1):

| Job                              | Swatinem | sccache (Rust) |
|----------------------------------|----------|----------------|
| Storage Verification Harness     | hit      | 76.92% |
| Engine Verification Harness 2/2  | hit      | (in flight at sample) |
| External Provider Tests (mysql)  | hit      | (high) |
| External Provider Tests (libsql) | **miss** | **0.00%** |

The libsql shard shows `Cache Key:` with no following `Cache hit
for:` / `Restored from cache key "..." full match: true` line,
which is the Swatinem signature for a complete miss. The same job
then reports 0.00% sccache Rust hit rate, meaning every rustc
invocation cold-compiled.

The variance — 77% on one harness lane, 0% on another — is the
key reason retiring `warm-sccache` is unsafe right now. Without
the warm pass, the libsql shard's cold-compile cost would land on
the PR critical path every run.

## Why the libsql shard misses Swatinem

Open question (deferred to a future PW-style wave). The cache key
for `External Provider Tests (libsql)` differs from the
`(mysql)` / `(postgres)` siblings in some way that prevents
restoration. Candidates worth investigating in the deferred wave:

- The libsql shard uses the same `shared-key` family in the
  composite action, but matrix variants may end up with
  per-matrix-leg cache keys due to Swatinem's path-aware hashing.
- The `--features libsql` cargo features (if any in the shard's
  test command) change the resolved dependency tree and therefore
  the hash.
- The libsql Docker fixture's side effects on the workspace
  (mtimes, generated content) may invalidate Swatinem's
  workspace-state component.

The PW1 docker-image cache lane is orthogonal: it caches the
**image**, not the rustc cache. The libsql shard still pays full
cold-compile cost on its rust deps, which is exactly what
`warm-sccache` was added (CC5) to mitigate.

## Wall-time implication

Pole pre-PW1+PW2+PW3:

| Pole                        | CW5 cost | After PW1+PW2+PW3 |
|-----------------------------|----------|-------------------|
| libsql gate dominance       | 17m 45s  | ≤ 10m (warm libsql image) |
| coverage on PR wall         | 26m 40s  | not on PR |
| concurrent-runner saturation | 27m queue wait | freed slots from coverage off-path |
| warm-sccache prelude        | 6m 16s   | retained (PW4c) |

After PW1+PW2+PW3 with `warm-sccache` retained, the PR wall floor
is roughly:

```
ui-artifacts (0m 27s) → warm-sccache (6m 16s) → harness_max (~11m) → gate_summary
```

= ~18m. This matches the PW4c condition-8 threshold (≤ 18m).

## What would unlock PW4b in a future wave

A targeted PW-style wave can address the Swatinem cache-miss
seam directly:

1. Instrument every cargo job's Swatinem invocation with a step
   that dumps `actions/cache` resolution details and the Swatinem
   key (already partially logged; needs structured emission).
2. Run 30 PR-shaped pushes back-to-back; compute per-job hit rate.
3. For any job with hit rate < 75%, audit why and either
   normalize the key (often a `Cargo.lock` regeneration on
   matrix-leg) or split it out.
4. Once every gate-path job sustains ≥ 85% Swatinem hit rate,
   retire `warm-sccache:` and confirm wall stays ≤ 15m on 3
   consecutive PR runs.

That wave is deferred — it requires more measurement than this
plan budgeted and would land its own active plan.

## Verifier

Condition 7 passes after PW4c:

```
[7] warm-sccache decision documented (PW4b retired OR PW4c retained with rationale)
  PASS  warm-sccache retained with PW4c rationale comment
```

Condition 8 wall threshold flips to 18m because `warm-sccache:`
remains present in `ci.yml`:

```
WALL_LIMIT_MIN=18
```

(See `scripts/verify-ci-pr-wall-sub-15.sh:246-250`.)

PW5 will collect 3 consecutive PR-branch runs each ≤ 18m wall.
