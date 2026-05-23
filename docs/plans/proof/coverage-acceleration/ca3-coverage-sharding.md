# CA3 — shard Coverage across 3 lanes with cargo llvm-cov reducer

Restructures the Coverage job from a single `runs-on: ubuntu-24.04`
job that compiles + tests + reports the full workspace into a 3-shard
fan-out + dependent reducer. Critical path goes from `sum(crates)` to
`max(shard) + reducer`.

## Shape

Before CA3:

```yaml
coverage:
  needs: [ui-artifacts, warm-sccache]
  steps:
    - setup, services, libsql start
    - cargo llvm-cov -j 4 --workspace --exclude nimbus-runtime --lcov --output-path lcov.info
    - upload + codecov
```

After CA3:

```yaml
coverage:
  needs: [ui-artifacts, warm-sccache]
  strategy:
    fail-fast: false
    matrix:
      include:
        - shard: server   # nimbus-server (heavy, external provider tests)
        - shard: engine   # nimbus-engine + nimbus-sandbox + nimbus-machine
        - shard: rest     # nimbus-core + nimbus-storage + nimbus-testing + nimbus-bin + nimbus
  steps:
    - setup, services, libsql (only when matrix.needs-providers == 'true')
    - cargo llvm-cov --no-report -j 4 ${{ matrix.packages }}
    - upload coverage-profraw-${{ matrix.shard }} artifact

coverage-reduce:
  needs: [coverage, ui-artifacts]
  steps:
    - setup (same shared-key, warm sccache hits)
    - cargo llvm-cov --no-run --workspace --exclude nimbus-runtime
    - download every coverage-profraw-* artifact into target/llvm-cov-target/profraw/
    - cargo llvm-cov report --lcov --output-path lcov.info
    - upload + codecov
```

## Shard partition rationale

Workspace member groups chosen to balance shard wall-clock:

| Shard | Crates | Why |
|-------|--------|-----|
| `server` | `nimbus-server` | Heaviest single crate; carries the bulk of the integration test surface that needs postgres/mysql/libsql fixtures. Sharding it off in isolation keeps the fixture-startup cost outside the other lanes. |
| `engine` | `nimbus-engine`, `nimbus-sandbox`, `nimbus-machine` | Middle-tier crates with substantial test counts but no external provider dependencies. |
| `rest` | `nimbus-core`, `nimbus-storage`, `nimbus-testing`, `nimbus-bin`, `nimbus` | Lightweight tail — types/validation crate, storage primitives, test helpers, CLI binary, facade. Each individually small; combined fits one shard. |

`nimbus-runtime` stays excluded workspace-wide — its instrumentation
budget was retired in CC6.

## Provider fixtures: conditional startup

The `services:` block (postgres, mysql) runs unconditionally on every
shard because GitHub Actions doesn't support matrix-conditional
services. The libsql startup, however, is in `steps:` and is gated by
`if: matrix.needs-providers == 'true'` — only the `server` shard pays
the libsql + namespace-probe wait cost.

A future optimization (CA5 follow-up): peel off the provider-bound
test set into a `provider-tests` shard distinct from `server` so most
shards skip even the postgres/mysql startup. Deferred because the
current shape is already a substantial improvement and the marginal
gain from peeling services is small relative to the link-time savings.

## Reducer mechanics

`cargo llvm-cov report` needs:

1. The instrumented binaries with their LLVM coverage map sections.
2. The `.profraw` files emitted at test runtime.

(1) is rebuilt on the reducer via `cargo llvm-cov --no-run --workspace`.
With sccache hits on every per-crate compilation unit (the shards
already populated the GHA-backed sccache for the same shared-key), this
step is cheap — most crates resolve to cache hits.

(2) is downloaded from every shard's `coverage-profraw-${shard}`
artifact into the reducer's `target/llvm-cov-target/profraw/`
directory. `actions/download-artifact@v8` with `pattern:
coverage-profraw-*` and `merge-multiple: true` flattens every shard's
profraw files into the same target directory; cargo-llvm-cov's report
mode picks up every `.profraw` file in that directory and merges
their counts.

The `--lcov --output-path lcov.info` then emits the unified lcov fragment
that codecov consumes downstream.

## Expected wall-clock delta

Baseline (post-CA1+CA2, before sharding): Coverage job ~13-17m at -j 4
under mold (extrapolated from ca0-baseline.md's 24m27s pre-mold -j 1
figure; not yet measured under the new conditions).

Post-CA3 estimate:

| Shard | Compile (warm sccache) | Test wall | Total |
|-------|------------------------|-----------|-------|
| server | 3-4m | 4-6m | 7-10m |
| engine | 2-3m | 2-3m | 4-6m |
| rest | 1-2m | 1-2m | 2-4m |

Critical path = max(server) = ~7-10m for the shard fan-out.

Reducer: ~3-5m (rebuild via warm sccache + profraw merge + lcov gen +
codecov upload).

**Total CA3 critical path: ~10-15m**, vs ~13-17m for the unsharded
post-CA1+CA2 baseline, vs 24m27s for the original pre-CA1 baseline.

The pre-CA1 → post-CA3 reduction is the headline: **24m27s → ~10-15m**.
Combined with sccache + Swatinem warmth from main, PR runs should see
Coverage stay under 12 minutes routinely.

Actual numbers will be appended after the first 3-5 post-CA3 runs on
main.

## Verifier delta

Before CA3:
- Condition 6 (Coverage sharded with reducer): FAIL —
  `shard_matrix=0 reducer=0`

After CA3:
- Condition 6: PASS — `Coverage job declares shard matrix and reducer
  calls cargo llvm-cov report`

The verifier regex was extended in this commit to recognize the
`matrix.include: [- shard: <value>, - shard: ...]` form alongside the
inline `shard: [a, b, c]` array form, since the per-shard `packages`
and `needs-providers` parameters require the include-list shape.

## Risk: artifact corruption from shard merging

If two shards' profraw files have overlapping coverage maps that
reference different binaries, the merged report could under-count or
miscount lines for shared modules. cargo-llvm-cov's profraw merging
is built on llvm-profdata which is robust to this case — it deduplicates
by source-file + function and merges hit counts. Shared workspace
dependencies (e.g. types from `nimbus-core`) get their hit counts
summed across shards rather than overwritten.

If a regression in coverage % shows up post-CA3 that's clearly a
sharding artifact rather than a real test deletion, fall back to the
pre-CA3 single-job shape; CA1+CA2's savings stand independently.
