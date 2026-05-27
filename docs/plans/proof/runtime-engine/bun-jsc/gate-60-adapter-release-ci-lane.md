# Gate 60: Bun/JSC Adapter Release CI Lane

Date: 2026-05-25

## Scope

This proof closes `BJD3` of
`docs/plans/archive/bun-jsc-distribution-and-release-plan.md`.

The goal is to add a source-backed adapter artifact lane without moving the
heavy Bun/WebKit build into default PR CI.

## Contract

Default PR CI remains lightweight:

- `bun-runtime-contract` still runs `make verify-bun-jsc-runtime-contract`.
- `proof-helpers` now syntax-checks every Bun/JSC adapter script.
- `proof-helpers` now runs
  `bash scripts/verify-bun-jsc-adapter-package-helper.sh`, which verifies the
  deterministic package/archive verifier against a good fixture and rejection
  cases.

Heavy artifact production is isolated to
`.github/workflows/bun-jsc-adapter.yml`:

- manual `workflow_dispatch`
- Linux x86_64 on `ubuntu-24.04`
- macOS arm64 on `macos-14`
- Bun source checkout defaults to `nimbus/bun`
  `bun-v1.4.0-nimbus.5`
- expected Bun revision defaults to
  `ad0e1d2bbc6690651e04f10eaf1dcdf8a6c0de57`
- each matrix lane runs `scripts/build-bun-jsc-adapter-artifacts.sh`
- each lane uploads the adapter archive, manifest, checksums, and proof logs

The workflow intentionally avoids an implicit curl installer. Operators can run
it on a runner image with a pinned Bun CLI already installed, or dispatch with
`install_bun_with_npm=true` and a recorded `bun_bootstrap_version`.

## Source-Backed Wrapper

`scripts/build-bun-jsc-adapter-artifacts.sh` is the release-lane composition
root:

1. Validates the Bun checkout is at the expected ref/revision and clean.
2. Rejects cross-target artifact builds until a separate cross-build proof
   exists.
3. Runs `scripts/verify-bun-jsc-linked-adapter.sh`.
4. Packages the produced shared library with
   `scripts/package-bun-jsc-adapter.sh`.
5. Verifies the archive with
   `scripts/verify-bun-jsc-adapter-package.sh`.
6. Writes `proof-summary-<platform>.txt` with the source, target, archive,
   digest, manifest, and log paths.

`scripts/verify-bun-jsc-linked-adapter.sh` now accepts
`NIMBUS_BUN_EXECUTABLE`, so release/proof runners can use an explicit pinned
Bun CLI path instead of relying on ambient `PATH`.

## Verification

Recorded local checks:

```text
bash -n scripts/bun-jsc-adapter-contract.sh \
  scripts/build-bun-jsc-adapter-artifacts.sh \
  scripts/package-bun-jsc-adapter.sh \
  scripts/verify-bun-jsc-adapter-package.sh \
  scripts/verify-bun-jsc-adapter-package-helper.sh \
  scripts/verify-bun-jsc-linked-adapter.sh

make verify-bun-jsc-adapter-package

git diff --check
```

The heavy source-backed workflow definition is not executed by default PR CI.
Full Linux/macOS source-backed execution remains the `BJD8` installed-package
proof gate.
