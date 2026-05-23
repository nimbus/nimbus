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

After CA3 (with CA5 hotfixes applied):

```yaml
coverage:
  needs: [ui-artifacts, warm-sccache]
  strategy:
    fail-fast: false
    matrix:
      include:
        - shard: server   # nimbus-server (heavy, libsql + postgres/mysql)
        - shard: engine   # nimbus-engine + nimbus-sandbox + nimbus-machine
                          # (libsql_replica_provider tests in nimbus-engine)
        - shard: rest     # nimbus-core + nimbus-storage + nimbus-testing + nimbus-bin + nimbus
                          # (libsql_provider tests in nimbus-storage)
  steps:
    - setup, services (postgres/mysql), libsql (unconditional — every shard needs it)
    - cargo llvm-cov --no-report -j 4 ${{ matrix.packages }}
    - upload coverage-profraw-${{ matrix.shard }} artifact
      (path: target/llvm-cov-target/*.profraw)

coverage-reduce:
  needs: [coverage, ui-artifacts]
  steps:
    - setup (same shared-key, warm sccache hits)
    - cargo llvm-cov --no-run --workspace --exclude nimbus-runtime
    - download every coverage-profraw-* artifact into target/llvm-cov-target/
      (merge-multiple: true)
    - cargo llvm-cov report --lcov --output-path lcov.info
    - upload + codecov
```

Three CA5 hotfixes converged on the current shape. CA3 originally
shipped a per-shard `needs-providers` flag gating libsql startup,
and the reducer used the deprecated `cargo llvm-cov --no-run`
incantation; three iterations were needed to fully unbreak the
shard pipeline.

1. **Profraw upload/download path (CA5 hotfix 1, `0d7b868e`).**
   cargo-llvm-cov writes profraw files into the target directory root
   (`target/llvm-cov-target/nimbus-<pid>-<m>.profraw`), not into a
   `profraw/` subdirectory. The original CA3 commit uploaded from
   `target/llvm-cov-target/profraw/` (which did not exist) and
   downloaded into the same nonexistent path. Upload now uses
   `path: target/llvm-cov-target/*.profraw`; the reducer downloads
   into `target/llvm-cov-target/` directly with `merge-multiple: true`,
   matching the location `cargo llvm-cov report` reads from.
2. **Engine shard libsql dependency (CA5 hotfix 1, `0d7b868e`).**
   `nimbus-engine` carries the `libsql_replica_provider` test family
   which opens the libsql admin API at `http://127.0.0.1:18081`. The
   original CA3 commit set `needs-providers: "false"` for the engine
   shard, gating off the libsql fixture and producing six panics in
   the `libsql_replica_provider` tests. Hotfix 1 flipped `engine` to
   `true`.
3. **Rest shard libsql dependency (CA5 hotfix 2).** Post-hotfix-1
   CI surfaced eight more panics on the `rest` shard:
   `nimbus-storage` carries its own `libsql_provider` test family
   that also requires the libsql admin API. With every current shard
   carrying libsql-dependent tests, the `needs-providers` flag is
   dead weight — hotfix 2 retires the flag and makes libsql startup
   unconditional. Future shards default to running libsql; only
   peel it off after measuring that no member crate needs it.
4. **Reducer rebuild incantation (CA5 hotfix 3).** Post-hotfix-2
   CI run 26321565127 had all three shards green but failed on the
   reducer's `Rebuild instrumented workspace (no run)` step.
   `cargo llvm-cov --no-run --workspace --exclude nimbus-runtime`
   produced `error: failed to merge profile data: not found
   *.profraw files in target/llvm-cov-target`. Current cargo-llvm-cov
   deprecates the `--no-run` flag and now interprets it as "merge
   already-collected profile data," not "just build." Hotfix 3
   switches to the documented pattern:
   `source <(cargo llvm-cov show-env --export-prefix); cargo test
   --no-run --workspace --exclude nimbus-runtime -j 4`. This exports
   the `LLVM_PROFILE_FILE`/`RUSTFLAGS` env that llvm-cov sets
   internally and lets `cargo test --no-run` build the instrumented
   binaries without attempting any merge.

## Shard partition rationale

Workspace member groups chosen to balance shard wall-clock:

| Shard | Crates | Why |
|-------|--------|-----|
| `server` | `nimbus-server` | Heaviest single crate; carries the bulk of the integration test surface that needs postgres/mysql/libsql fixtures. Sharding it off in isolation keeps the per-crate cost outside the other lanes. |
| `engine` | `nimbus-engine`, `nimbus-sandbox`, `nimbus-machine` | Middle-tier crates with substantial test counts. `nimbus-engine` carries `libsql_replica_provider` tests that need the libsql admin API. |
| `rest` | `nimbus-core`, `nimbus-storage`, `nimbus-testing`, `nimbus-bin`, `nimbus` | Lightweight tail — types/validation, storage primitives, test helpers, CLI binary, facade. `nimbus-storage` carries `libsql_provider` tests that also need the libsql admin API. |

`nimbus-runtime` stays excluded workspace-wide — its instrumentation
budget was retired in CC6.

## Provider fixtures: unconditional startup

The `services:` block (postgres, mysql) runs unconditionally on every
shard because GitHub Actions doesn't support matrix-conditional
services. The libsql startup also runs on every shard — every
current shard carries at least one libsql-dependent test family.
CA3 originally tried a `needs-providers` per-shard flag to peel
libsql off the lighter shards, but two hotfix iterations
(`engine`, then `rest`) discovered every shard needs it. The flag is
retired; the libsql start + wait steps are now unconditional.

A future optimization: peel off the provider-bound
test set into a `provider-tests` shard distinct from `server` so most
shards skip even the postgres/mysql startup. Deferred because the
current shape is already a substantial improvement and the marginal
gain from peeling services is small relative to the link-time savings.

## Reducer mechanics

`cargo llvm-cov report` needs:

1. The instrumented binaries with their LLVM coverage map sections.
2. The `.profraw` files emitted at test runtime.

(1) is rebuilt on the reducer with the show-env + cargo-test
pattern:

```sh
source <(cargo llvm-cov show-env --export-prefix)
cargo test --no-run --workspace --exclude nimbus-runtime -j 4
```

`cargo llvm-cov --no-run` is deprecated in current cargo-llvm-cov
and now tries to merge profile data instead of just building, which
fails on the reducer because the profraws are downloaded *after*
the rebuild step. The show-env approach exports the
`LLVM_PROFILE_FILE`/`RUSTFLAGS` env that cargo-llvm-cov would
otherwise set internally and lets `cargo test --no-run` build the
instrumented binaries without attempting any merge. With sccache
hits on every per-crate compilation unit (the shards already
populated the GHA-backed sccache for the same shared-key), this
step is cheap — most crates resolve to cache hits.

(2) is downloaded from every shard's `coverage-profraw-${shard}`
artifact into the reducer's `target/llvm-cov-target/` directory
(not a `profraw/` subdirectory; profraws live in the target dir
root). `actions/download-artifact@v8` with `pattern:
coverage-profraw-*` and `merge-multiple: true` flattens every
shard's profraw files into the same target directory;
cargo-llvm-cov's report mode picks up every `.profraw` file in
that directory and merges their counts.

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
