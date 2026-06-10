# Gate 67: Enterprise Hardening Follow-Up

Date: 2026-05-25

## Scope

This gate closes the post-closeout audit findings for the optional in-process
Bun/JSC adapter distribution baseline:

- Runtime diagnostics discover and verify adapter metadata without eager
  `dlopen`; the shared adapter is loaded only on Bun/JSC invocation, and failed
  loads are not cached for the process lifetime.
- Linux packaged adapter discovery rejects unsafe package paths through a
  root-owned, non-group/other-writable trust chain.
- The direct Linux installer reuses the strict adapter contract: archive
  layout, checksums, manifest source/ABI/memory/lifecycle, SBOM/SLSA evidence,
  exact exports, native-symbol leak checks, and late-`dlopen` safety checks.
- Package and release checksum verifiers exact-match checksum subjects instead
  of substring-matching lines.
- Manual Bun/JSC adapter artifact workflow dispatches install the pinned Bun
  CLI by default so hosted runners are self-sufficient.

## Verification

Local macOS:

- `bash -n scripts/install.sh scripts/verify-bun-jsc-adapter-package.sh scripts/verify-bun-jsc-release-assets.sh scripts/verify-bun-jsc-adapter-package-helper.sh scripts/verify-bun-jsc-release-assets-helper.sh scripts/verify-install-helper.sh` passed.
- `dash -n scripts/install.sh` passed.
- `bash scripts/verify-bun-jsc-adapter-package-helper.sh` passed, including
  checksum subject spoofing rejection.
- `bash scripts/verify-bun-jsc-release-assets-helper.sh` passed, including
  release checksum subject spoofing rejection.
- `bash scripts/verify-install-helper.sh` passed 35 tests.
- `cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib backends::bun_jsc -- --nocapture` passed 32 tests.
- `make verify-bun-jsc-runtime-contract` passed all 7 stages after running
  outside the local sandbox for test listener binding: 11 runtime policy tests,
  10 Bun/JSC backend tests, 15 registry tests, 2 runtime metrics API tests,
  1 tenant admission test, and 5 operator UI tests.
- `cargo fmt --all --check` passed.
- `git diff --check` passed.

Linux minicloud:

- Temporary disposable worktrees were created from local commit `072545f8` plus
  this patch set; the main minicloud checkout was not modified.
- `git diff --check`, Bash syntax checks, and `dash -n scripts/install.sh`
  passed.
- `bash scripts/verify-bun-jsc-adapter-package-helper.sh` passed.
- `bash scripts/verify-bun-jsc-release-assets-helper.sh` passed.
- `bash scripts/verify-install-helper.sh` passed 37 tests on Debian 13.
- After installing a user-scoped Rust stable toolchain for the `nimbus` user
  (`rustc 1.93.1`), `CARGO_TARGET_DIR=~/src/github.com/nimbus/nimbus/target cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib backends::bun_jsc::manifest -- --nocapture` passed 20 tests, including the Linux-only packaged trust-chain rejection.

Temporary minicloud verification bundles, logs, worktrees, and refs were
removed after the proof run.
