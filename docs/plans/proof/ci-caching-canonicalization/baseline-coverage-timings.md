# CC baseline — Coverage job timings (pre-sccache)

Snapshot captured 2026-05-21 immediately before CC1 (sccache to
Coverage only) starts. This is the "before" state for the
ci-caching-canonicalization plan; CC1's proof at
`cc1-coverage-only-stats.md` will quote these numbers when reporting
the delta.

## Three consecutive runs on `main`

| Run ID         | HEAD     | Trigger          | Cache restore  | Generate coverage report | Wallclock (job total) | Conclusion |
|----------------|----------|------------------|----------------|---------------------------|------------------------|------------|
| 26208672632    | a4c52f5e | push             | warm (Swatinem hit) | 8m 27s              | ~25m 00s              | success    |
| 26213464606    | 44065b6c | `gh run rerun --failed` | warm           | 29m 05s (29min ceiling — see note) | ~26m 24s          | success    |
| 26232434845    | 4a958173 | push             | **cold** (1s "cache not found") | 22m 11s    | ~24m 33s              | success (1)|

(1) The CI run as a whole was marked `failure` because of an unrelated
`runtime::tests::cooperative::cooperative_concurrent_dispatch_does_not_deadlock_subprocess`
flake in `Runtime Verification Harness`. The Coverage job itself
completed `success`; the 22m 11s number is correct for the cold-cache
build under the bumped 45-min timeout.

## Cache state on 2026-05-21

`gh api repos/nimbus/nimbus/actions/caches` (saved verbatim at
`baseline-cache-state.json`) showed 11 active caches totaling 32.49 GB.
Nine of those are 3-4 GB Rust workspace slots, all storing
near-identical dep content:

```
clippy
deny
runtime-tests
external-providers
harness-storage
harness-engine
harness-server
harness-runtime
desktop-ui
```

The **Coverage cache slot is absent**. The previous green Coverage was
the rerun on 44065b6c, which hit a warm cache from the original failed
run, made no observable change to `target/`, and Swatinem's
"save-if-changed" heuristic skipped the save. Next push (4a958173)
restored a 1-second "cache not found" line and cold-built the full
instrumented workspace — surviving only because LD7's cleanup commit
bumped Coverage's `timeout-minutes` from 30 to 45.

This is the central evidence for CC3 (Swatinem `save-always: true`).

## Step-by-step breakdown for the cold run (4a958173)

| Step                              | Duration | Notes                                |
|-----------------------------------|----------|--------------------------------------|
| Check out repository              | 3s       |                                      |
| Install Rust toolchain            | <1s      |                                      |
| Cache cargo artifacts             | 1s       | **MISS** — cache slot absent         |
| Install cargo-llvm-cov            | 1s       |                                      |
| Set up Node.js                    | 4s       |                                      |
| Install JS dependencies           | 9s       |                                      |
| Build nimbus-ui artifacts         | 10s      | (duplicated across 6 jobs — CC4)     |
| Start libsql provider fixture     | 4s       |                                      |
| Generate coverage report          | **22m 11s** | cold cargo-llvm-cov full instrumented build |
| Upload coverage artifact          | 1s       |                                      |
| Upload coverage to Codecov        | 4s       |                                      |
| Post Cache cargo artifacts (save) | 68s      |                                      |
| **Total job**                     | **~24m 33s** | (Started 14:29:55Z, ended 14:54:28Z) |

## What this baseline supports

- **CC1 hit-rate measurement.** After CC1, on push N (cold sccache),
  the "Generate coverage report" step should land in the same 22-24
  min range (within sccache miss overhead); on push N+1 it should
  drop to 8-12 min with the warm sccache.
- **CC3 rerun-safety regression test.** The rerun on 44065b6c is the
  reproducer: trigger a CI run, `gh run rerun --failed`, verify the
  post-rerun cache state matches expectations.
- **CC4 ui-artifacts savings.** "Set up Node.js" + "Install JS
  dependencies" + "Build nimbus-ui artifacts" = 23s in Coverage
  alone, multiplied across 6 jobs that today do this work
  in parallel = 2-3 min wall-clock waste per push.
- **CC6 -j N re-test.** The 22m 11s cold "Generate coverage report"
  step is what we measure against; `-j 2` and higher should chop a
  meaningful fraction of link time if rust-lld no longer surfaces
  bus errors.
- **CC7 --no-doc-tests savings.** Doc-test instrumentation rebuilds
  crates as `--test` harnesses; baseline `Generate coverage report`
  on cold cache is 22m 11s; CC7 measures the delta.

## Concurrency context

A push to `main` fires three workflows simultaneously:

- `ci.yml` (9 Rust jobs)
- `desktop-ui.yml` (1 Rust job — `desktop-ui-smoke`)
- `verify-nimbus-crun-patch.yml` (no Rust compilation)

All ten Rust jobs cold-build identical workspace dep trees in parallel
on push N. CC5's `warm-sccache` leader job converts this to
serial-cold-then-parallel-warm.

## Sources

- `gh run list --branch main --workflow=CI --limit 3 --json conclusion,status,headSha,databaseId,createdAt,updatedAt`
- `gh run view <id> --json jobs -q '.jobs[] | select(.name == "Coverage") | .steps[]'`
- `gh api repos/nimbus/nimbus/actions/caches?per_page=100` (saved as `baseline-cache-state.json`)
