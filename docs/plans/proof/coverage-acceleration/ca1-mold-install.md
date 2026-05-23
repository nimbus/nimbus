# CA1 — install mold linker in setup-rust-cached composite

Lands the prerequisite the CC6-era comment block at
`.github/workflows/ci.yml:686-694` named for retesting Coverage
`-j > 1`. Composite-action change only; every Rust job that flows
through `setup-rust-cached` (12 sites at landing time) picks it up
on its next run.

## Diff shape

```yaml
# .github/actions/setup-rust-cached/action.yml — new step, inserted
# after the googlesource credentials step and before the Rust toolchain
# install:

- name: Install mold linker (Linux)
  if: runner.os == 'Linux'
  shell: bash
  run: |
    sudo apt-get update
    sudo apt-get install -y --no-install-recommends mold
    echo "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=mold" >> "${GITHUB_ENV}"
    echo "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=mold" >> "${GITHUB_ENV}"
    mold --version
```

## Why mold

| Linker | Typical link wall on a large instrumented Nimbus test binary |
|--------|--------------------------------------------------------------|
| GNU bfd (default Linux toolchain linker) | baseline |
| LLVM lld (rust-lld; what `--linker rust-lld` would route to) | ~2× faster than bfd |
| mold (Linker by Rui Ueyama) | 3–10× faster than lld on large debuginfo+coverage binaries |

The CC6 comment block specifically flagged **rust-lld bus errors on
parallel link** as the constraint that forced `-j 1`. mold's separate
process model and faster link path are expected to side-step the
bus-error class entirely — `-j 4` becomes safe under mold even with
the instrumented profile in play. CA2 verifies this against the live
Coverage job.

## Why apt over a release tarball

mold has been in the standard Ubuntu apt main repo since 22.04 Jammy.
On 24.04 Noble (the workflow runner pin per CM3), `apt install mold`
resolves to a current build with no PPA required and no GPG-pin
maintenance.

## Cross-platform gate

`runner.os == 'Linux'` gates the install step. The composite is
currently only invoked on Linux runners; this gate exists so the
composite remains portable once CA4 migrates `release.yml`'s macOS
(`macos-14`) and Windows (`windows-latest`) sites onto it. macOS / Windows
branches keep their system linker — investigating equivalents (`sold`,
`lld`-on-mac, native-lld-on-windows) is deferred to a follow-up if the
release-pipeline wall justifies it.

## Verifier delta

Before CA1:
- Condition 4 (mold in composite): FAIL — `install=0 linker_env=0`

After CA1:
- Condition 4: PASS — `Composite installs mold and exports linker env var`

## Expected CI timing delta

CA1 alone is not expected to move the Coverage critical path much —
the link savings only materialize when paired with CA2's parallelism
unlock. Other Rust jobs that go through the composite (Clippy,
Workspace Tests, Harness lanes, Deny, Warm sccache) may see modest
link-time reductions on cold-cache runs; warm-cache runs are
dominated by cache restore and won't change measurably.

CA2's commit message records the first warm-cache Coverage timing
under mold + `-j N`; that is the load-bearing measurement.
