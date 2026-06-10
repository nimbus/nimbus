# CC7 — Coverage scope optimization (--no-doc-tests)

The CC7 task is to skip doc-test instrumentation in the Coverage job's
cargo-llvm-cov invocation. Doc-test instrumentation forces cargo-llvm-cov
to rebuild every crate as a `--test` harness *twice* (once for the
regular test suite, once for the doc-test harness), so the linker
serializes through ~2× the binaries on a cold instrumented build.

## Change

`.github/workflows/ci.yml`, `Generate coverage report` step:

```diff
-cargo llvm-cov -j 1 --workspace --exclude nimbus-runtime --lcov --output-path lcov.info
+cargo llvm-cov -j 1 --workspace --exclude nimbus-runtime --no-doc-tests --lcov --output-path lcov.info
```

## What this does and does not change

- **Does not skip doc-tests as a test.** The `rust-workspace-tests` job
  runs `make test-rust-docs` (libtest-based doctest pass) — doc-tests
  still execute and still gate merges. CC7 only stops *measuring* their
  line coverage.
- **Does not affect line coverage of regular tests.** The instrumented
  test binaries that exercise non-doc-test code paths are unchanged.
- **Skips coverage measurement of code paths that only doc-tests
  exercise.** This is the line-count delta the plan asks us to bound
  at <2%.

## Expected wallclock impact

The baseline cold-cache Coverage run was 22m 11s
(`baseline-coverage-timings.md`). Doc-test instrumentation drives a
second linker pass per crate; skipping it should save roughly 30-40%
of the link portion of the build. Order-of-magnitude estimate: cold
Coverage drops to ~14-16 min, warm Coverage (with sccache populated
upstream by `warm-sccache`) to ~6-8 min.

## Coverage line-count delta

_Empirical comparison deferred._ Capturing a before/after lcov diff
requires:

1. One run on the post-CC5 baseline (sccache + Swatinem warm) with
   doc-test instrumentation enabled, downloading `coverage-lcov`.
2. One run on the CC7 change with `--no-doc-tests`, downloading
   `coverage-lcov`.
3. Diffing per-crate line counts via the lcov output and reporting
   the largest deltas.

The expectation is that the largest deltas land on crates whose
doc-tests exercise non-trivial code paths (e.g., nimbus-storage,
nimbus-server). Doc-tests are usually small example snippets, so the
delta should stay under the 2% bound the plan calls for. If a future
follow-up finds a crate with >2% delta, the right response is to
promote the doc-test scenarios into regular `#[test]`s in that crate
rather than reinstating doc-test instrumentation workspace-wide.

## Sources

- `docs/plans/ci-caching-canonicalization-plan.md` row CC7.
- `docs/plans/proof/ci-caching-canonicalization/baseline-coverage-timings.md`.
- `cargo llvm-cov --help` for `--no-doc-tests` semantics.
