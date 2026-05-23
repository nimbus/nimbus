#!/usr/bin/env bash
# Runs the Bun/JSC in-process lockdown proof gate.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUN_REPO="${NIMBUS_BUN_REPO:-${HOME}/src/github.com/oven-sh/bun}"
BUN_BUILD_DIR="${NIMBUS_BUN_BUILD_DIR:-/private/tmp/nimbus-bun-embed-native}"
BUN_CACHE_DIR="${NIMBUS_BUN_CACHE_DIR:-/private/tmp/nimbus-bun-cache}"
BUN_RUST_ONLY_BUILD_DIR="${NIMBUS_BUN_RUST_ONLY_BUILD_DIR:-/private/tmp/nimbus-bun-rust-only}"
BUN_CARGO_TARGET_DIR="${NIMBUS_BUN_CARGO_TARGET_DIR:-/private/tmp/nimbus-bun-proof-target}"

if [[ ! -d /private/tmp ]]; then
  BUN_BUILD_DIR="${NIMBUS_BUN_BUILD_DIR:-/tmp/nimbus-bun-embed-native}"
  BUN_CACHE_DIR="${NIMBUS_BUN_CACHE_DIR:-/tmp/nimbus-bun-cache}"
  BUN_RUST_ONLY_BUILD_DIR="${NIMBUS_BUN_RUST_ONLY_BUILD_DIR:-/tmp/nimbus-bun-rust-only}"
  BUN_CARGO_TARGET_DIR="${NIMBUS_BUN_CARGO_TARGET_DIR:-/tmp/nimbus-bun-proof-target}"
fi

cd "${REPO_ROOT}"

printf 'Bun/JSC in-process lockdown gate\n'
printf 'Nimbus repo: %s\n' "${REPO_ROOT}"
printf 'Bun repo:    %s\n\n' "${BUN_REPO}"

if [[ ! -f "${BUN_REPO}/src/jsc/Cargo.toml" ]]; then
  printf 'missing Bun checkout: expected %s/src/jsc/Cargo.toml\n' "${BUN_REPO}" >&2
  printf 'set NIMBUS_BUN_REPO to the local Bun worktree and rerun\n' >&2
  exit 1
fi

printf '[1/10] Nimbus format\n'
cargo fmt --all --check

printf '\n[2/10] Nimbus UI build prerequisites\n'
if [[ ! -f node_modules/.package-lock.json ]]; then
  npm ci
fi
make build-ui

printf '\n[3/10] Nimbus runtime/backend policy tests\n'
cargo test -p nimbus-runtime limits::tests --lib

printf '\n[4/10] Nimbus registry/runtime metadata rejection tests\n'
cargo test -p nimbus-server registry_and_license::registry --lib

printf '\n[5/10] Nimbus runtime diagnostics tests\n'
cargo test -p nimbus-server registry_and_license::runtime_metrics --lib

printf '\n[6/10] Nimbus ignored Bun source proof lane\n'
NIMBUS_BUN_REPO="${BUN_REPO}" \
NIMBUS_BUN_BUILD_DIR="${BUN_RUST_ONLY_BUILD_DIR}" \
NIMBUS_BUN_CACHE_DIR="${BUN_CACHE_DIR}" \
NIMBUS_BUN_CARGO_TARGET_DIR="${BUN_CARGO_TARGET_DIR}" \
cargo test -p nimbus-runtime --test engine_proofs \
  bun_jsc_build_gate_reproduces_from_bun_build_graph \
  -- --ignored --nocapture

printf '\n[7/10] Nimbus whitespace diff check\n'
git diff --check

printf '\n[8/10] Bun Rust format\n'
(cd "${BUN_REPO}" && cargo fmt --all --check)

printf '\n[9/10] Bun native embed probe\n'
(cd "${BUN_REPO}" && bun scripts/build.ts --profile=debug-no-asan \
  --build-dir="${BUN_BUILD_DIR}" \
  --cache-dir="${BUN_CACHE_DIR}" \
  --target=check-bun-embed-probe)

printf '\n[10/10] Bun whitespace diff check\n'
(cd "${BUN_REPO}" && git diff --check)

printf '\nBun/JSC in-process lockdown gate: pass\n'
printf '\nProduct promotion requires this gate to stay green on macOS and Linux/minicloud with NIMBUS_BUN_REPO pointing at the current Bun proof worktree or future Nimbus Bun fork.\n'
