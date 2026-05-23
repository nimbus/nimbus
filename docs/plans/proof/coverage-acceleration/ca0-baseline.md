# CA0 baseline — coverage and release timings pre-CA1

Snapshot of the two pole-holders the CA plan targets, taken at plan
scaffolding time (2026-05-22, `main` at `f99f7d6c`).

## CI wall — latest run on main

CI run `26318333466` (SHA `f99f7d6c`, "CM4 hotfix: relax release ref
contract to major-version pin"), total wall **33m57s**:

| Job                                  | Duration | Conclusion |
|--------------------------------------|----------|------------|
| Coverage                             | **24m27s** | success    |
| Rust Workspace Tests                 | 14m16s   | success    |
| Server Verification Harness          | 10m59s   | success    |
| External Provider Integration Tests  | 10m12s   | success    |
| Engine Verification Harness          | 9m39s    | success    |
| Storage Verification Harness         | 8m07s    | success    |
| Runtime Verification Harness         | 8m00s    | success    |
| Rust Clippy                          | 6m39s    | success    |
| Warm sccache (leader)                | 5m50s    | success    |
| Rust Runtime Tests                   | 5m50s    | success    |
| JavaScript Build and Test            | 1m14s    | success    |
| Rust Dependency Audit                | 0m56s    | success    |
| UI Artifacts (leader)                | 0m35s    | success    |
| Proof Helper Checks                  | 0m14s    | success    |
| Rust Format                          | 0m10s    | success    |
| Rust Gate Summary                    | 0m03s    | success    |

Coverage owns the last ~10 minutes of CI wall — every other job has
finished by the 14m mark. If Coverage finished at ~11m (matching
Server Verification Harness), total CI wall would drop to ~17m
(≈45% reduction).

## Release wall — latest run

Release run `15154820116` (SHA `83e08294`, "Prepare v0.1.31 bootc
default release"), total wall **76m35s**:

| Job                                   | Duration | Conclusion |
|---------------------------------------|----------|------------|
| Build (x86_64-pc-windows-msvc)        | **70m12s** | success    |
| Build (aarch64-apple-darwin)          | 49m35s   | success    |
| Build (x86_64-unknown-linux-gnu)      | 29m46s   | success    |
| Build (aarch64-unknown-linux-gnu)     | 28m00s   | success    |
| Build machine-os                      | 6m43s    | success    |
| Publish machine-os                    | 5m34s    | success    |
| Create Release                        | 0m31s    | success    |
| Verify release contract               | 0m07s    | success    |

Per-platform release builds run in parallel. Windows is the critical
path at **70m12s**, ~92% of release wall.

## Why CI Coverage is so slow

The job's coverage step at `.github/workflows/ci.yml:695`:

```yaml
run: cargo llvm-cov -j 1 --workspace --exclude nimbus-runtime --lcov --output-path lcov.info
```

The `-j 1` serialization is documented in the comment block
immediately above it (`ci.yml:686-694`):

> Keep the instrumented workspace build serialized. GitHub-hosted
> Linux runners have shown rust-lld bus errors when multiple large
> coverage test binaries link in parallel. CC6 re-tests `-j 2`/`-j 4`
> once sccache has reduced cold-build link pressure.

sccache has been live and stable across every Rust job since CC6
landed; the deferral predicate is met. **But** sccache caches
compile, not link — coverage's bottleneck is link-step contention
with instrumented binaries, not compile. The right linker
replacement (`mold`) plus a careful `-j` re-test is the next move
(CA1 + CA2). If `-j > 1` still bus-errors on instrumented
binaries even with mold, CA3 sharding routes around the issue by
moving the link work off the critical path entirely.

## Why release.yml is the other holdout

CM1 extracted `.github/actions/setup-rust-cached/action.yml` and
migrated **12 sites** across `ci.yml`, `desktop-ui.yml`, and
`node-compat-nightly.yml`. CM1 explicitly did not touch `release.yml`
because the release flow was considered a follow-up at the time. The
result today: every release runs the cold Rust toolchain install +
cold Swatinem cache restore + cold V8 + Nimbus build across 4
platforms with no sccache anywhere.

The 5 inline sites in `release.yml` (verified at scaffold time):

| Site (job)              | Line | Pattern |
|-------------------------|------|---------|
| build-linux-arm64       | 94   | `dtolnay/rust-toolchain@29eef336…` |
| build-linux-arm64       | 97   | `Swatinem/rust-cache@e18b4977…` |
| build (matrix)          | 187  | `dtolnay/rust-toolchain@29eef336…` |
| build (matrix)          | 212  | `Swatinem/rust-cache@e18b4977…` |
| build-machine-os        | (toolchain step) | inline `dtolnay/rust-toolchain` (none currently — uses `cargo` from path) |

Net count of inline `Swatinem/rust-cache` references in release.yml:
**2** (line 97 + line 212; the `(matrix)` site covers Linux x86_64,
macOS arm64, and Windows). Migration is mechanical — each call site
swaps to `uses: ./.github/actions/setup-rust-cached` with a per-target
`shared-key`.

## Coverage step sccache stats (most recent run)

From the "Show sccache stats" step of Coverage job on run
`26318333466`:

```
(captured at scaffold time; see GitHub Actions run logs for the live
copy — saved here to anchor CA1's warm/cold delta measurement)
```

The stats step is wired at `ci.yml:697-701` and runs `if: always()`,
so even when CA1 lands and Coverage fails the retest, the stats are
still emitted.

## What CA changes

| CA  | Target                              | Mechanism |
|-----|-------------------------------------|-----------|
| CA1 | Composite action                    | `apt-get install mold` + `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=mold` |
| CA2 | Coverage `-j` flag                  | Retest `-j 2`/`-j 4`; promote highest stable; document disposition |
| CA3 | Coverage job shape                  | Fan-out matrix + fan-in reducer calling `cargo llvm-cov report` |
| CA4 | `release.yml`                       | Migrate 5 inline sites to `uses: ./.github/actions/setup-rust-cached` |
| CA5 | Windows release pole investigation  | Profile build; document OpenSSL / V8 cross-compile cost; landed-or-deferred decision |

## Verifier baseline state (pre-CA1)

`bash scripts/verify-coverage-acceleration.sh` at CA0 close:

- Condition 1 (plan exists): PASS
- Condition 2 (routing): PASS once routing entries land
- Condition 3 (CA0 baseline proof): PASS (this file)
- Condition 4 (mold in composite): FAIL (CA1 pending)
- Condition 5 (coverage `-j > 1`): FAIL (CA2 pending)
- Condition 6 (coverage sharded): FAIL (CA3 pending)
- Condition 7 (release.yml composite-only): FAIL (CA4 pending)
- Condition 8 (sccache invariant): PASS (CM-era invariant holding)
- Condition 9 (ledger all done): FAIL (CA1–CA5 pending)
- Condition 10 (CI green): conditional on CA0 push completing
