# Bun/JSC Gate 22: Reproducible Verification Lane

Date: 2026-05-23

Nimbus plan: `docs/plans/archive/bun-jsc-in-process-lockdown-plan.md`

Script: `scripts/verify-bun-jsc-in-process-lockdown.sh`

## Decision

Status: local and Linux/minicloud verification lane added.

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

On Linux, the script falls back from `/private/tmp` to `/tmp`. For small
tmpfs-backed hosts, set the scratch directories under a home-backed path such
as `~/.cache/nimbus-proof`.

## Script Gates

| Step | Command family | Purpose |
| --- | --- | --- |
| 1 | `cargo fmt --all --check` | Nimbus formatting baseline. |
| 2 | `npm ci` when needed, then `make build-ui` | Builds the embedded UI prerequisite required by `nimbus-server` in clean checkouts. |
| 3 | `cargo test -p nimbus-runtime limits::tests --lib` | Runtime backend/trust/lockdown/lifecycle policy rejection tests. |
| 4 | `cargo test -p nimbus-server registry_and_license::registry --lib` | Server/runtime metadata rejection coverage. |
| 5 | `cargo test -p nimbus-server registry_and_license::runtime_metrics --lib` | Runtime diagnostics shape coverage, including lifecycle policy. |
| 6 | ignored `engine_proofs` Bun source lane | Reproduces the Rust-only Bun source proof through the local Bun checkout. |
| 7 | `git diff --check` in Nimbus | Whitespace safety. |
| 8 | `cargo fmt --all --check` in Bun | Bun proof formatting baseline. |
| 9 | `bun scripts/build.ts --profile=debug-no-asan --target=check-bun-embed-probe` | Native Bun embed probe covering construct/destroy, host calls, generated wrapper, timeout/cancel, permission inventory, memory, package policy, and lifecycle reuse. |
| 10 | `git diff --check` in Bun | Bun whitespace safety. |

## Linux / Minicloud Promotion Lane

Before any product promotion or fork dependency, the same gate must run on a
Linux host with:

```sh
NIMBUS_BUN_REPO=~/src/github.com/oven-sh/bun \
bash scripts/verify-bun-jsc-in-process-lockdown.sh
```

That lane is required because Bun/JSC embed behavior touches native linking,
JSC/WebKit, process/thread lifecycle, timers, filesystem, and networking code
that can differ across macOS and Linux.

On 2026-05-23 this gate passed on the Debian 13 `minicloud` host after setting
up only user-local toolchains:

- Rust stable via `rustup`: `rustc 1.95.0 (59807616e 2026-04-14)`
- Node LTS via `nvm`: `v24.16.0`; npm `11.13.0`
- Bun user-local: `1.3.14`
- LLVM user-local: `21.1.8`, from the official
  `LLVM-21.1.8-Linux-X64.tar.xz` release asset verified with
  `sha256:b3b7f2801d15d50736acea3c73982994d025b01c2f035b91ae3b49d1b575732b`
- Nimbus scratch root: `~/.cache/nimbus-proof`

The Linux lane also proved two harness requirements that should carry into any
future embedder API or pool:

- background threads that touch JSC termination must initialize Bun/WebKit
  stack bounds first
- cancellation proofs must avoid hair-trigger timing that can fire before the
  guest spin marker is reached in debug Linux builds

## Outcome

`BIL6` is complete. The script runs successfully on the macOS worktree and the
Linux/minicloud lane. Bun/JSC is still blocked at the permission and resolver
seams, so the Linux pass is proof evidence, not product promotion.
