#!/usr/bin/env bash
# Verifies the opt-in Bun/JSC linked-adapter build lane. This gate keeps the
# default Nimbus build fail-closed while checking the exact Bun proof source
# and native embedder target that a future BJA4 execution adapter will call.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUN_REPO="${NIMBUS_BUN_REPO:-${HOME}/src/github.com/oven-sh/bun}"
EXPECTED_BUN_REV="${NIMBUS_BUN_EXPECTED_REV:-2f09ba33b184a541e2ade24bf6e46bebc971a262}"

if [[ -d /private/tmp ]]; then
  BUN_BUILD_DIR="${NIMBUS_BUN_BUILD_DIR:-/private/tmp/nimbus-bun-linked-adapter}"
  BUN_CACHE_DIR="${NIMBUS_BUN_CACHE_DIR:-/private/tmp/nimbus-bun-cache}"
  BUN_CARGO_TARGET_DIR="${NIMBUS_BUN_CARGO_TARGET_DIR:-/private/tmp/nimbus-bun-proof-target}"
else
  BUN_PROOF_ROOT="${NIMBUS_BUN_PROOF_ROOT:-${XDG_CACHE_HOME:-${HOME}/.cache}/nimbus-bun-proof}"
  BUN_BUILD_DIR="${NIMBUS_BUN_BUILD_DIR:-${BUN_PROOF_ROOT}/linked-adapter}"
  BUN_CACHE_DIR="${NIMBUS_BUN_CACHE_DIR:-${BUN_PROOF_ROOT}/bun-cache}"
  BUN_CARGO_TARGET_DIR="${NIMBUS_BUN_CARGO_TARGET_DIR:-${BUN_PROOF_ROOT}/bun-cargo-target}"
fi

REQUIRED_EXPORTS=(
  nimbus_bun_embed_probe_construct_and_destroy_vm
  nimbus_bun_embed_probe_sync_host_call
  nimbus_bun_embed_probe_async_host_call
  nimbus_bun_embed_probe_program_bundle_host_calls
  nimbus_bun_embed_probe_timeout_and_cancel
  nimbus_bun_embed_probe_permission_surface_inventory
  nimbus_bun_embed_probe_memory_behavior
  nimbus_bun_embed_probe_package_module_policy
  nimbus_bun_embed_probe_lifecycle_reuse_stress
)

cd "${REPO_ROOT}"

printf 'Bun/JSC linked adapter gate\n'
printf 'Nimbus repo: %s\n' "${REPO_ROOT}"
printf 'Bun repo:    %s\n' "${BUN_REPO}"
printf 'Bun rev:     %s\n\n' "${EXPECTED_BUN_REV}"

if [[ ! -f "${BUN_REPO}/src/embed_probe/lib.rs" ]]; then
  printf 'missing Bun checkout: expected %s/src/embed_probe/lib.rs\n' "${BUN_REPO}" >&2
  printf 'set NIMBUS_BUN_REPO to the Bun proof worktree or future Nimbus Bun fork\n' >&2
  exit 1
fi

actual_bun_rev="$(git -C "${BUN_REPO}" rev-parse HEAD)"
if [[ "${actual_bun_rev}" != "${EXPECTED_BUN_REV}" ]]; then
  printf 'unexpected Bun revision: expected %s, got %s\n' \
    "${EXPECTED_BUN_REV}" "${actual_bun_rev}" >&2
  printf 'set NIMBUS_BUN_EXPECTED_REV only when the plan has recorded a new source baseline\n' >&2
  exit 1
fi

bun_status="$(git -C "${BUN_REPO}" status --short)"
if [[ -n "${bun_status}" ]]; then
  printf 'Bun proof worktree must be clean for linked-adapter verification:\n%s\n' \
    "${bun_status}" >&2
  exit 1
fi

printf '[1/7] Default no-link runtime contract\n'
make verify-bun-jsc-runtime-contract

printf '\n[2/7] Linked adapter feature compile and unit contract\n'
cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib backends::bun_jsc

printf '\n[3/7] Bun proof source exports\n'
for export in "${REQUIRED_EXPORTS[@]}"; do
  git -C "${BUN_REPO}" grep -q --fixed-strings "${export}" -- \
    src/embed_probe/lib.rs scripts/build/bun.ts
  printf '  %s\n' "${export}"
done

printf '\n[4/7] Bun Rust format\n'
(cd "${BUN_REPO}" && cargo fmt --all --check)

printf '\n[5/7] Bun native embed probe\n'
mkdir -p "${BUN_BUILD_DIR}" "${BUN_CACHE_DIR}" "${BUN_CARGO_TARGET_DIR}"
(cd "${BUN_REPO}" && CARGO_TARGET_DIR="${BUN_CARGO_TARGET_DIR}" \
  bun scripts/build.ts --profile=debug-no-asan \
    --build-dir="${BUN_BUILD_DIR}" \
    --cache-dir="${BUN_CACHE_DIR}" \
    --target=check-bun-embed-probe)

printf '\n[6/7] Nimbus whitespace diff check\n'
git diff --check

printf '\n[7/7] Bun whitespace diff check\n'
(cd "${BUN_REPO}" && git diff --check)

printf '\nBun/JSC linked adapter gate: pass\n'
