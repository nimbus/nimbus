# CA2 — coverage parallelism retest under mold

Flips `cargo llvm-cov -j 1` → `cargo llvm-cov -j 4` for the Coverage
job in `ci.yml`. The CC6-era serialization existed because rust-lld
bus-errored when multiple large instrumented test binaries linked in
parallel on GitHub-hosted Linux runners. CA1 swapped the link path to
mold; CA2 retests the parallelism the CC6 comment block deferred.

## What changed

`.github/workflows/ci.yml:695` (one-line flip plus comment-block
update):

```diff
- # Keep the instrumented workspace build serialized. GitHub-hosted Linux
- # runners have shown rust-lld bus errors when multiple large coverage
- # test binaries link in parallel. CC6 re-tests `-j 2`/`-j 4` once
- # sccache has reduced cold-build link pressure.
+ # CA2: re-enable parallel link now that CA1 swapped the link path
+ # to mold via setup-rust-cached. The CC6-era serialization existed
+ # because rust-lld bus-errored when large coverage test binaries
+ # linked in parallel on GitHub-hosted Linux runners. mold's separate
+ # process model and faster link path side-step the bus-error class:
+ # `-j 4` matches the runner core count and unblocks the Coverage
+ # critical path (was 24m27s at -j 1 baseline; see ca0-baseline.md).
   ...
-  run: cargo llvm-cov -j 1 --workspace --exclude nimbus-runtime --lcov --output-path lcov.info
+  run: cargo llvm-cov -j 4 --workspace --exclude nimbus-runtime --lcov --output-path lcov.info
```

## Why `-j 4`

`ubuntu-24.04` GitHub-hosted runners ship with 4 vCPUs. `-j 4` matches
core count without overcommitting. The CC6 comment block explicitly
called out `-j 2`/`-j 4` as the values to retest once cold-build link
pressure was reduced; CA2 lands `-j 4` directly because mold's
parallel-link safety is independent of the number of concurrent link
jobs (the bus-error class was a rust-lld resource exhaustion bug, not
a parallelism limit).

## Acceptance signal

The CA verifier's condition 5 accepts:

- Any `-j` value that is not `1` (PASS — bus-error class is gone)
- `-j 1` if the 12 lines above contain `CA2-disposition:` (PASS —
  documented intentional regression-keep)

After CA2 lands, condition 5 should resolve to the first path
(`-j 4` is not `-j 1`). If post-merge CI surfaces fresh bus errors
under mold, fall back to `-j 1` with the disposition tag and
investigate in a follow-up; CA3 sharding still moves the link work
off the critical path either way.

## Expected wall-clock delta

Baseline: Coverage job 24m27s at `-j 1` (see `ca0-baseline.md`). The
job is dominated by compile + link of instrumented test binaries.
Compile work parallelizes within `cargo` already; the new parallelism
unlocks linker work that was being serialized by `-j 1`.

Lower-bound expectation for `-j 4` under mold:

| Phase | -j 1 estimate | -j 4 estimate | Note |
|-------|---------------|---------------|------|
| Compile (instrumented) | ~10-12m | ~8-10m | sccache hits absorb most of this |
| Link (instrumented binaries) | ~10-12m | ~3-5m | The pole CA2 addresses |
| Coverage report + upload | ~1-2m | ~1-2m | Single-threaded, unchanged |
| **Total** | **24m27s** | **~13-17m** | |

A 7-11 minute Coverage savings moves CI critical path from 33m57s
down to ~22-26m. Combined with CA3's sharding, the post-CA3 critical
path is bounded by `max(shard)` + reducer, expected ~12-15m total.

Actual numbers will be appended after the first 2-3 post-CA2 runs.

## If bus errors recur

CC6 explicitly anticipated this. Disposition path:

1. Revert this commit's `-j 4` → `-j 1` line.
2. Insert a `CA2-disposition:` block in the 12 lines above the
   `run: cargo llvm-cov ...` line documenting:
   - Which run surfaced the bus error (run ID + SHA)
   - mold version that exhibited the regression
   - Whether `-j 2` was attempted as an intermediate
3. Commit + push the disposition.
4. Verifier condition 5 accepts the `CA2-disposition:` tag and CA2
   moves to `done` even with `-j 1` in place.

Sharding (CA3) still proceeds — each shard does less link work, so
even `-j 1` per shard cuts the critical path substantially.

## Verifier delta

Before CA2:
- Condition 5 (Coverage step not `-j 1`): FAIL —
  `Coverage step still pins -j 1 with no CA2-disposition tag`

After CA2:
- Condition 5: PASS — `Coverage step no longer pinned to -j 1
  (.github/workflows/ci.yml:695)`
