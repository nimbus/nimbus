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
          needs-providers: "true"
        - shard: engine
          packages: "-p nimbus-engine -p nimbus-sandbox -p nimbus-machine"
          needs-providers: "true"
        - shard: rest
          packages: "-p nimbus-core -p nimbus-storage -p nimbus-testing -p nimbus-bin -p nimbus"
          needs-providers: "false"
  steps:
    - cargo llvm-cov --no-report -j 4 ${{ matrix.packages }}
    - upload coverage-profraw-${{ matrix.shard }} artifact
      (path: target/llvm-cov-target/*.profraw)

coverage-reduce:
  needs: [coverage, ui-artifacts]
  steps:
    - cargo llvm-cov --no-run --workspace --exclude nimbus-runtime
    - download coverage-profraw-* artifacts (merge-multiple: true)
      to target/llvm-cov-target/
    - cargo llvm-cov report --lcov --output-path lcov.info
    - upload + codecov
```

Two CA3 path/dependency details are load-bearing and easy to get wrong:

- **Profraw files live in `target/llvm-cov-target/` directly**, not in
  a `profraw/` subdirectory. `cargo llvm-cov --no-report` writes them
  as `target/llvm-cov-target/nimbus-<pid>-<m>.profraw`. Upload with
  `path: target/llvm-cov-target/*.profraw`; download into
  `target/llvm-cov-target/` with `merge-multiple: true` so
  `cargo llvm-cov report` finds them at the path it expects.
- **The `engine` shard sets `needs-providers: "true"`** because
  `nimbus-engine`'s `libsql_replica_provider` tests need the libsql
  admin API. Only the `rest` shard skips libsql startup. The initial
  CA3 commit landed with `engine` set to `false`, which caused six
  `libsql_replica_provider` tests to panic with
  `error sending request for url (http://127.0.0.1:18081/.../create)`;
  the CA5 hotfix flipped it to `true`.

Shard partition rationale:

| Shard | Crates | Why |
|-------|--------|-----|
| `server` | `nimbus-server` | Heaviest single crate; carries the postgres/mysql/libsql integration surface. |
| `engine` | `nimbus-engine`, `nimbus-sandbox`, `nimbus-machine` | Middle-tier crates with substantial test counts; needs libsql for `nimbus-engine`'s `libsql_replica_provider` tests. |
| `rest` | `nimbus-core`, `nimbus-storage`, `nimbus-testing`, `nimbus-bin`, `nimbus` | Lightweight tail; combined fits one shard. |

`nimbus-runtime` stays excluded workspace-wide — its coverage
instrumentation budget was retired in CC6.

Provider fixtures (postgres, mysql) run unconditionally on every
shard because GitHub Actions does not support matrix-conditional
services. The libsql startup is gated on
`if: matrix.needs-providers == 'true'`; both `server` and `engine`
pay the libsql + namespace-probe wait because both carry
libsql-dependent tests, while `rest` skips it.

When adding a new workspace crate, place it in the shard that
balances wall-clock best (typically `rest` unless it pulls external
provider tests, in which case it joins `server`). Do not introduce
a 4th shard without measuring — the fan-out + reducer overhead has
diminishing returns past 3 lanes given current per-crate cost.

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
