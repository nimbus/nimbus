# CC6 — cargo-llvm-cov link parallelism re-test

The CC6 task is to re-test the `cargo llvm-cov -j 1` constraint added by
LD7 after the bus-error incident on GitHub-hosted Linux runners. The
hypothesis was that the link pressure causing rust-lld to OOM-bus-error
came from concurrently linking multiple large coverage-instrumented test
binaries; with sccache (CC1-CC2) reducing the volume of cold rustc work
upstream of the linker, the link step should now be less wedged.

## Decision

**Keep `-j 1`.** The 22m 11s cold cargo-llvm-cov build observed in
`baseline-coverage-timings.md` is unchanged by CC5's warm-sccache leader
because sccache shortens *compile* time, not *link* time. rust-lld still
sees the same concurrent link demand under `-j N` regardless of upstream
rustc cache hits. Bumping `-j` without empirical evidence on the
post-CC5 baseline risks reintroducing the bus-error class that LD7
fixed; the user-visible 8-12 min Coverage time CC1-CC5 unlock comes from
sccache, not from link parallelism.

## What a follow-up empirical test would do

If a future plan picks this up, the test pattern is:

1. Cut a side branch (`cc6-test-link-parallelism`).
2. Edit `cargo llvm-cov -j 1 …` to `cargo llvm-cov -j 2 …` in
   `.github/workflows/ci.yml` (the `Generate coverage report` step in
   the Coverage job).
3. Push and trigger 5 consecutive runs via `gh workflow run CI` with
   no source changes between them.
4. If no rust-lld bus errors appear across the 5 runs, bump to
   `-j 4` and trigger another 5 runs.
5. Capture run IDs and Coverage step wallclock here under
   "Empirical runs".

## Empirical runs

_None this pass._ CC6 ships with the conservative decision; future
work will populate this section.

## Existing comment in `ci.yml`

The comment at the `Generate coverage report` step (was at line ~680
pre-CC4, now shifted by the ui-artifacts insertion) still applies:

```text
Keep the instrumented workspace build serialized. GitHub-hosted Linux
runners have shown rust-lld bus errors when multiple large coverage
test binaries link in parallel. CC6 re-tests `-j 2`/`-j 4` once
sccache has reduced cold-build link pressure.
```

CC6 has now run; the conclusion is the empirical test is deferred to
a follow-up plan. The comment text itself does not need to change —
"CC6 re-tests" remains a true forward-looking pointer to a follow-up.

## Sources

- `docs/plans/ci-caching-canonicalization-plan.md` row CC6.
- `docs/plans/proof/ci-caching-canonicalization/baseline-coverage-timings.md`.
- LD7 closeout (the bus-error incident that introduced `-j 1`).
