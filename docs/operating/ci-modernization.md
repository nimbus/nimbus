# CI modernization contract

Canonical contract for the CI infrastructure that surrounds the
caching stack documented in `ci-caching.md`. Authored by the
CI Modernization (CM) plan; see
`docs/plans/archive/ci-modernization-plan.md` for the underlying
ledger and proof artifacts.

## Single composite action for Rust toolchain bootstrap

`.github/actions/setup-rust-cached/action.yml` is the only place the
following third-party actions are referenced:

- `dtolnay/rust-toolchain`
- `mozilla-actions/sccache-action`
- `Swatinem/rust-cache`

Every Rust job that needs a toolchain + sccache + cargo cache calls
the composite:

```yaml
- name: Set up Rust with cache
  uses: ./.github/actions/setup-rust-cached
  with:
    shared-key: <ci-ubuntu-stable-<job>-no-bin-v2>
    toolchain-components: <optional rustup components>
    cache-bin: <"false" by default; "true" only for desktop-ui>
    googlesource-cookie: ${{ secrets.GOOGLESOURCE_COOKIE }}
```

When the upstream actions need bumping, **edit one file**. Do not
inline `mozilla-actions/sccache-action` or `Swatinem/rust-cache` into
a job; if a new Rust job ships, route it through the composite.

The CC `save-if: refs/heads/main` gate and the `cache-bin: "false"`
default flow through the composite — see `ci-caching.md` for the
caching contract those settings enforce.

## SHA pinning policy

Every non-`actions/*` `uses:` reference in `.github/workflows/*.yml`
and `.github/actions/**/action.yml` MUST be:

1. A 40-char hex commit SHA.
2. Followed by a comment on the same line or within the next two
   lines containing the human-readable version tag (e.g.
   `# v0.0.10`, `# stable`, `# v4`).

Dependabot keeps SHA pins fresh; the version-name comment is what
makes the diff legible in the PR.

`actions/*` first-party references may stay tag-pinned, but only at
major granularity (`@v3`, not `@v3.2.0`). Patch pinning blocks
Dependabot from flowing security patches.

Three-segment refs like `github/codeql-action/init@<sha>` are
allowed; the verifier regex accepts nested action sub-paths.

## Runner pinning

Use explicit Ubuntu LTS image tags. **Never** use the moving alias
`ubuntu-latest`. Today's pin is `ubuntu-24.04` (plus
`ubuntu-24.04-arm` for ARM jobs). `ubuntu-22.04` may still appear
where ABI compatibility with the release archive requires it
(e.g. `release.yml::build` for `x86_64-unknown-linux-gnu`); document
the reason inline if you keep an older image.

## Job summaries

High-value jobs emit a structured `$GITHUB_STEP_SUMMARY` markdown
block so the workflow-run page is a useful triage surface. The
established shape:

```bash
{
  echo "## <job name>"
  echo
  echo "**Status:** :white_check_mark: passed"   # or :x:, :no_entry:
  echo
  echo "<details><summary>Output (last N lines)</summary>"
  echo
  echo '```'
  tail -n N /tmp/out
  echo '```'
  echo
  echo "</details>"
} >> "${GITHUB_STEP_SUMMARY}"
```

Use `:white_check_mark:` / `:x:` / `:no_entry:` / `:fast_forward:`
for status icons. Bound embedded log output (`tail -n 80` is a
sensible default). Defensive: wrap in `if: always()` and check
output files exist so a failed earlier step still emits its
summary.

Jobs currently emitting summaries: `deny`, `coverage`,
`rust-gate-summary` (ci.yml); `desktop-ui` (desktop-ui.yml).

## Static analysis (CodeQL)

`.github/workflows/codeql.yml` runs `github/codeql-action/init` and
`github/codeql-action/analyze` (SHA-pinned per CM2) over a matrix of
JavaScript/TypeScript (build-mode `none`) and Rust (build-mode
`manual`, using `make check` as the build step). Schedule: push to
main, PR targeting main, weekly cron, manual dispatch.

Results land in the GitHub Security tab. False positives can be
tuned later via `.github/codeql/codeql-config.yml`; no config file is
required for the default `security-and-quality` query suite.

## Dependabot

`.github/dependabot.yml` covers `cargo`, `github-actions`, and `npm`
with a weekly Monday cadence and per-ecosystem grouping. The
`github-actions` ecosystem is the one that surfaces SHA bumps for
the third-party actions in the composite and one-off workflow sites.
PRs preserve the `# vX.Y.Z` comment.

## Aggregate gate

`bash scripts/verify-ci-modernization.sh` is the local equivalent of
the closeout check; it exits 0 iff all 12 conditions of the original
CM plan are satisfied. The CC plan has its own gate at
`scripts/verify-ci-caching-canonicalization.sh`. Both gates can be
run independently; CM2 is composite-aware on the CC side, so neither
regresses the other.

The Coverage Acceleration (CA) plan that followed has its own gate
at `scripts/verify-coverage-acceleration.sh`; the three gates layer
without conflict.

## Coverage and release acceleration

The Coverage Acceleration (CA) plan, executed on the CM/CC baseline,
landed four contract additions that future contributors should
preserve:

### mold linker on Linux

The composite action installs `mold` on every Linux runner and
exports the linker via `RUSTFLAGS`:

```yaml
- name: Install mold linker (Linux)
  if: runner.os == 'Linux'
  shell: bash
  run: |
    sudo apt-get update
    sudo apt-get install -y --no-install-recommends mold
    echo "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS=-C link-arg=-fuse-ld=mold" >> "${GITHUB_ENV}"
    echo "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS=-C link-arg=-fuse-ld=mold" >> "${GITHUB_ENV}"
    mold --version
```

Do not switch this to `CARGO_TARGET_*_LINKER=mold` — that invocation
makes rustc call mold directly via the gcc driver protocol, which
passes `-m64`, which mold rejects with `fatal: unknown -m argument:
64`. The `RUSTFLAGS+fuse-ld=mold` path keeps `cc` as the driver and
delegates to mold for the link step, which is the supported shape.

macOS and Windows branches of the composite no-op this step; they
remain on the platform default linker. Use `runner.os == 'Linux'`
to gate the install.

### Coverage `-j` constant

`cargo llvm-cov` in `ci.yml` runs with `-j 4` (matching `ubuntu-24.04`
core count). The CC6-era `-j 1` serialization existed to avoid a
rust-lld bus-error class triggered by parallel link of large
instrumented test binaries; mold's separate process model side-steps
that class.

If a future runner image change resurrects the bus-error class:

1. Revert the Coverage step's `-j` to `-j 1`.
2. Insert a `CA2-disposition:` block in the 12 lines above the
   `run: cargo llvm-cov ...` line documenting which run surfaced
   the regression and mold version that exhibited it.

The verifier's condition 5 accepts both shapes; the disposition tag
keeps the contract intentional rather than drifty.

### Coverage sharding shape (fan-out + reducer)

The Coverage job in `ci.yml` is a 3-shard fan-out + dependent
reducer rather than a single workspace-wide job:

```yaml
coverage:
  strategy:
    fail-fast: false
    matrix:
      include:
        - shard: server
          packages: "-p nimbus-server"
        - shard: engine
          packages: "-p nimbus-engine -p nimbus-sandbox -p nimbus-machine"
        - shard: rest
          packages: "-p nimbus-core -p nimbus-storage -p nimbus-testing -p nimbus-bin -p nimbus"
  steps:
    - source <(cargo llvm-cov show-env --export-prefix)
    - cargo test ${{ matrix.packages }} -j 4
    - upload coverage-profraw-${{ matrix.shard }} artifact
      (path: target/nimbus-*.profraw)

coverage-reduce:
  needs: [coverage, ui-artifacts]
  steps:
    - source <(cargo llvm-cov show-env --export-prefix)
    - cargo test --no-run --workspace --exclude nimbus-runtime -j 4
    - download coverage-profraw-* artifacts (merge-multiple: true)
      to target/
    - source <(cargo llvm-cov show-env --export-prefix)
    - cargo llvm-cov report --lcov --output-path lcov.info
    - upload + codecov
```

Three CA3 path/dependency details are load-bearing and easy to get wrong:

- **Every cargo-llvm-cov invocation goes through `show-env`.** Both
  the shard run and the reducer source
  `<(cargo llvm-cov show-env --export-prefix)` before invoking
  `cargo test` or `cargo llvm-cov report`. Profraws live in
  `target/nimbus-*.profraw` (show-env's
  `LLVM_PROFILE_FILE=target/nimbus-%p-%12m.profraw` default).
  Mixing show-env on the reducer with `cargo llvm-cov --no-report`
  on the shards does not work — `--no-report` sets
  `CARGO_TARGET_DIR=target/llvm-cov-target` internally while
  show-env leaves `CARGO_TARGET_DIR` unset, so the two modes write
  to different target-dir layouts. Upload with
  `path: target/nimbus-*.profraw`; download into `target/` with
  `merge-multiple: true` so the show-env-driven
  `cargo llvm-cov report` finds them at the path it expects.
- **Every shard runs the libsql fixture.** `nimbus-engine` carries
  `libsql_replica_provider` tests; `nimbus-storage` carries
  `libsql_provider` tests. All three shards (`server`, `engine`,
  `rest`) need the libsql admin API at `127.0.0.1:18081`. CA3 first
  shipped a per-shard `needs-providers` flag that gated libsql
  startup on a subset; two follow-up hotfixes converged on
  always-on libsql. The flag is now retired — the libsql start +
  wait steps are unconditional.
- **The reducer rebuilds with `show-env` + `cargo test --no-run`**,
  not `cargo llvm-cov --no-run`. The latter is deprecated in current
  cargo-llvm-cov and now tries to merge profraw data instead of just
  building, which fails on the reducer because the profraws are
  downloaded *after* the rebuild step. The supported pattern is to
  source the `LLVM_PROFILE_FILE`/`RUSTFLAGS` env via
  `cargo llvm-cov show-env --export-prefix` and then run
  `cargo test --no-run`, which compiles the instrumented binaries
  without attempting any merge.

Shard partition rationale:

| Shard | Crates | Why |
|-------|--------|-----|
| `server` | `nimbus-server` | Heaviest single crate; carries the postgres/mysql/libsql integration surface. |
| `engine` | `nimbus-engine`, `nimbus-sandbox`, `nimbus-machine` | Middle-tier crates with substantial test counts; `nimbus-engine` carries `libsql_replica_provider` tests. |
| `rest` | `nimbus-core`, `nimbus-storage`, `nimbus-testing`, `nimbus-bin`, `nimbus` | Lightweight tail; `nimbus-storage` carries `libsql_provider` tests. |

`nimbus-runtime` stays excluded workspace-wide — its coverage
instrumentation budget was retired in CC6.

Provider fixtures (postgres, mysql) run unconditionally on every
shard because GitHub Actions does not support matrix-conditional
services. The libsql startup also runs on every shard — every
current shard carries at least one libsql-dependent test family
(`server` has nimbus-server's integration surface, `engine` has
`libsql_replica_provider`, `rest` has `libsql_provider` via
`nimbus-storage`).

When adding a new workspace crate, place it in the shard that
balances wall-clock best (typically `rest` unless it pulls a heavy
provider surface, in which case it joins `server`). Do not
introduce a 4th shard without measuring — the fan-out + reducer
overhead has diminishing returns past 3 lanes given current
per-crate cost.

### release.yml composite adoption

Every Rust build site in `release.yml` (5 sites: `build-linux-arm64`
plus the 3-entry `build` matrix covering linux-x86_64, darwin-arm64,
windows-x86_64) routes through the composite the same way PR CI jobs
do. The composite's mold step is Linux-gated and no-ops on macOS
and Windows.

Release tag builds opt into `save-cache: always`:

```yaml
- uses: ./.github/actions/setup-rust-cached
  with:
    shared-key: release-${{ matrix.target }}-no-bin-v1
    save-cache: always
    googlesource-cookie: ${{ secrets.GOOGLESOURCE_COOKIE }}
```

The composite's `save-cache` input controls the Swatinem `save-if`:

| `save-cache` | Behavior |
|--------------|----------|
| `auto` (default) | Save only on `refs/heads/main` (PR CI invariant). |
| `always` | Save on any ref (release tags use this). |
| `never` | Disable saves entirely. |

The CC9 retraction (`save-if: refs/heads/main`) protects PR CI from
poisoning main's caches; `save-cache: always` on release tags lets
per-target caches accumulate across tags without violating that
invariant (release tags do not run from `refs/pull/*`).

The `shared-key: release-<target>-no-bin-v1` namespace keeps release
tag caches isolated from PR CI's `ci-ubuntu-stable-<job>-no-bin-v2`
namespace. Bump the `-v1` suffix to invalidate on schema-incompatible
cache changes (e.g. composite-action contract changes that break
restore semantics).

### Aggregate gate

`bash scripts/verify-coverage-acceleration.sh` is the local
equivalent of CA's closeout check; it exits 0 iff all 10 conditions
of the CA plan are satisfied. The CA gate layers cleanly on top of
the CM and CC gates.

## PR critical-path acceleration

The CW (CI Wall Acceleration) wave attacks the PR-side wall poles that
remained after CA collapsed the Coverage pole. The CW0 baseline on
`32951ee7` measured a 23m34s wall whose critical path was
`warm-sccache (10.2m) → Server Verification Harness (12.7m)` plus lateral
poles at Rust Workspace Tests (15.7m) and External Provider Integration
Tests (14.6m). The four CW lanes target each pole directly.

### CW1: harness corpus sharding

`scripts/verification-harness.sh` accepts an optional third positional
argument of the form `N/M`. When passed, the script exports
`NIMBUS_HARNESS_SHARD=N/M` to the cargo invocation; the in-test corpus
filter at `crates/nimbus-storage/src/simulation/verification.rs`,
`crates/nimbus-server/src/tests/verification_harness.rs`, and
`crates/nimbus-runtime/src/runtime/tests/verification_harness.rs`
honors the env var by selecting only cases with `index % M == N - 1`.

The `harness` job matrix in `.github/workflows/ci.yml` expands per
surface: server runs 4 shards (its 7-case transport-liveness corpus
dominates), engine runs 2 shards (2-case generated-history corpus),
storage stays single-shard (already 8.3m, below the wall pole), runtime
stays single-shard (1.4m). Server harness max shard drops from 12.7m
to ~3.5m.

### CW2: workspace tests sharding via nextest `--partition`

`Makefile`'s `test-rust-workspace` target reads `NIMBUS_NEXTEST_PARTITION`
and forwards it as `--partition hash:N/M` to `cargo nextest run`. The
`rust-workspace-tests` job in `ci.yml` expands into a 3-shard matrix
with `partition: "1/3" | "2/3" | "3/3"`. Doctests (not supported by
nextest) stay pinned to shard 1 via `if: matrix.run-doctests == 'true'`.

`nextest --partition hash:N/M` hashes test paths so the partition is
stable across runs (cache reuse + retry-on-failure both behave) but
unpredictable enough to balance load across shards (~639/702/670 in
local validation).

### CW3: external-provider tests per-provider matrix

`scripts/test-external-providers.sh` reads `NIMBUS_PROVIDER_FILTER` and
dispatches to `postgres | mysql | libsql`, each running only that
provider's nimbus-storage + nimbus-engine cargo invocations. When the
filter is empty, behavior matches the pre-CW3 sequential script.

The `external-provider-tests` job in `ci.yml` expands into a 3-shard
matrix on `provider` and starts only its provider's docker fixture via
`if: matrix.provider == '<name>'` startup steps. The pre-CW3
`services:` block is retired; postgres + mysql now use the same
`docker run` shape as libsql, with `--health-cmd` / `--health-interval`
preserving the previous health-gating semantics.

`fail-fast: false` ensures one provider's failure does not cancel the
other two — `needs['external-provider-tests'].result` in
`rust-gate-summary` still aggregates to a single result for the gate.

### Bun/JSC optional backend contract gate

The `bun-runtime-contract` job runs
`make verify-bun-jsc-runtime-contract`. It is intentionally a Nimbus-side
contract gate, not the full Bun source proof. The lane verifies that:

- Bun/JSC lane metadata is admitted only for the proven fresh/discard,
  outer-quota-required profile.
- Bun/JSC executors stay lazy and `execution_adapter_state` remains
  `not_linked` until a real adapter is linked.
- V8 and Node compatibility lanes keep `v8_isolate_heap_limit` memory
  semantics and do not inherit Bun/JSC backend axes from resource overrides.
- `/debug/runtime/metrics` and the operator settings UI render the same lane
  order and memory-enforcement contract.

The heavier `scripts/verify-bun-jsc-in-process-lockdown.sh` gate still owns
the Bun source proof and must pass on macOS and Linux/minicloud before product
promotion.

### CW4: warm-sccache compile-cost reduction

CW4's `warm-sccache` job in `ci.yml` runs `cargo check --workspace`
(was `cargo check --workspace --tests` pre-CW4). The `--tests` drop is
the landed lane.

**Why drop `--tests`.** With `--tests`, the warm pass rustc-compiles
every integration test binary across the workspace. Each test binary
is its own rustc invocation with its own dev-dep mix, so the sccache
keys are distinct from anything downstream test jobs would emit on
their own. Downstream test jobs (harness shards, coverage shards,
workspace-tests shards) cold-compile their own test bins anyway when
they cargo-test the relevant crates; warm-sccache populating those keys
upstream produced cross-job reuse only on the second downstream job to
run a given test surface — and the harness/coverage/workspace jobs are
already sharded by CW1 / CA3 / CW2 so each shard's test-bin compile is
small and parallelized across runners.

The dep-graph compile that *does* benefit from cross-job reuse (lib
crates + their transitive deps) is unaffected: lib/bin rustc calls
produce the same sccache keys downstream test jobs hit when they
cargo-test those same crates.

**Deferred lane: per-target Swatinem cache layer.** The CW plan
considered a second lane prototyping a per-target Swatinem cache slot
for `warm-sccache` so its `target/` would be restored between runs.
On inspection this is redundant: `Swatinem/rust-cache@v2` already
caches `target/` (with built-in filtering to exclude bloat), and the
composite at `.github/actions/setup-rust-cached/action.yml` wires
that v2 invocation onto `warm-sccache` via the shared-key
`ci-ubuntu-stable-warm-sccache-no-bin-v2`. The 10.2m CW0 baseline
already reflects target/ being restored. A bespoke additional layer
would need to identify what *isn't* cached and measure savings — a
research lane that requires CI-run measurement we did not run here.
Defer until a measured win justifies the added complexity.

### Aggregate gate

`bash scripts/verify-ci-wall-acceleration.sh` is the local equivalent
of CW's closeout check; it exits 0 iff all 10 conditions of the CW
plan are satisfied. The CW gate layers cleanly on top of the CM, CC,
and CA gates.

### Deferred follow-up: Windows release pole

The CA5 closeout (see `docs/plans/proof/coverage-acceleration/ca5-closeout.md`)
identifies three structural cost components on the
`x86_64-pc-windows-msvc` release build (vendored OpenSSL via
Strawberry Perl, V8 prebuilt link via `link.exe`, cold-target
`cargo build --release`) and lists candidate lanes for a future
release-acceleration plan: `rustls` over OpenSSL, `lld-link` over
`link.exe`, and a release-side `warm-sccache` leader.

Promote a new active plan before attacking these — none are in
scope for the closed CA wave.

## Routing

- Canonical contract: this file (`docs/operating/ci-modernization.md`).
- Plan archive: `docs/plans/archive/ci-modernization-plan.md`,
  `docs/plans/archive/coverage-acceleration-plan.md`.
- Proof artifacts: `docs/plans/proof/ci-modernization/`,
  `docs/plans/proof/coverage-acceleration/`.
- Verifier: `scripts/verify-ci-modernization.sh`,
  `scripts/verify-coverage-acceleration.sh`.
- Sister contract (caching layer): `docs/operating/ci-caching.md`.
