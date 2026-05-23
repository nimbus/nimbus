# Bun/JSC Gate 22: Reproducible Verification Lane

Date: 2026-05-23

Nimbus plan: `docs/plans/bun-jsc-in-process-lockdown-plan.md`

Script: `scripts/verify-bun-jsc-in-process-lockdown.sh`

## Decision

Status: local verification lane added.

The Bun/JSC in-process lockdown proof now has a single Nimbus-side script that
reruns the current proof set without making Bun/JSC selectable.

## Local Command

```sh
bash scripts/verify-bun-jsc-in-process-lockdown.sh
```

Optional environment:

```sh
NIMBUS_BUN_REPO=/Users/jack/src/github.com/oven-sh/bun
NIMBUS_BUN_BUILD_DIR=/private/tmp/nimbus-bun-embed-native
NIMBUS_BUN_CACHE_DIR=/private/tmp/nimbus-bun-cache
NIMBUS_BUN_RUST_ONLY_BUILD_DIR=/private/tmp/nimbus-bun-rust-only
NIMBUS_BUN_CARGO_TARGET_DIR=/private/tmp/nimbus-bun-proof-target
```

On Linux, the script falls back from `/private/tmp` to `/tmp`.

## Script Gates

| Step | Command family | Purpose |
| --- | --- | --- |
| 1 | `cargo fmt --all --check` | Nimbus formatting baseline. |
| 2 | `cargo test -p nimbus-runtime limits::tests --lib` | Runtime backend/trust/lockdown/lifecycle policy rejection tests. |
| 3 | `cargo test -p nimbus-server registry_and_license::registry --lib` | Server/runtime metadata rejection coverage. |
| 4 | `cargo test -p nimbus-server registry_and_license::runtime_metrics --lib` | Runtime diagnostics shape coverage, including lifecycle policy. |
| 5 | ignored `engine_proofs` Bun source lane | Reproduces the Rust-only Bun source proof through the local Bun checkout. |
| 6 | `git diff --check` in Nimbus | Whitespace safety. |
| 7 | `cargo fmt --all --check` in Bun | Bun proof formatting baseline. |
| 8 | `bun scripts/build.ts --profile=debug-no-asan --target=check-bun-embed-probe` | Native Bun embed probe covering construct/destroy, host calls, generated wrapper, timeout/cancel, permission inventory, memory, package policy, and lifecycle reuse. |
| 9 | `git diff --check` in Bun | Bun whitespace safety. |

## Linux / Minicloud Promotion Lane

Before any product promotion or fork dependency, the same gate must run on the
Linux minicloud host with:

```sh
NIMBUS_BUN_REPO=~/src/github.com/oven-sh/bun \
bash scripts/verify-bun-jsc-in-process-lockdown.sh
```

That lane is required because Bun/JSC embed behavior touches native linking,
JSC/WebKit, process/thread lifecycle, timers, filesystem, and networking code
that can differ across macOS and Linux.

## Outcome

`BIL6` is complete when the script runs successfully on this macOS worktree.
The Linux/minicloud lane remains a named promotion prerequisite, not a current
product gate, because Bun/JSC is still blocked at the permission and resolver
seams.
