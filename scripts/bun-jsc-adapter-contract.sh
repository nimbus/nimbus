#!/usr/bin/env bash
# Shared shell-side contract for the optional Nimbus Bun/JSC shared adapter.
#
# Keep this file aligned with
# crates/nimbus-runtime/src/backends/bun_jsc/manifest.rs. Runtime tests assert
# the Rust-side source/export contract; shell verifiers source this file so
# packaging, release, and source-backed proof lanes do not drift separately.

if [[ -n "${NIMBUS_BUN_JSC_ADAPTER_CONTRACT_SH_INCLUDED:-}" ]]; then
  return 0 2>/dev/null || exit 0
fi
NIMBUS_BUN_JSC_ADAPTER_CONTRACT_SH_INCLUDED=1

BUN_JSC_ADAPTER_SCHEMA_VERSION=1
BUN_JSC_ADAPTER_KIND="nimbus.bun_jsc.adapter"
BUN_JSC_ADAPTER_ABI_NAME="nimbus-bun-jsc-embedder"
BUN_JSC_ADAPTER_ABI_VERSION=2
BUN_JSC_ADAPTER_MEMORY_ENFORCEMENT="outer_quota_required"
BUN_JSC_ADAPTER_LIFECYCLE="fresh_discard"
BUN_JSC_ADAPTER_MANIFEST_FILE="nimbus-bun-jsc-adapter.json"
BUN_JSC_ADAPTER_CHECKSUMS_FILE="checksums-sha256.txt"
BUN_JSC_ADAPTER_README_FILE="README.md"
BUN_JSC_ADAPTER_SOURCE_REPOSITORY="https://github.com/nimbus/bun"
BUN_JSC_ADAPTER_SOURCE_REF="codex/bun-v1.4.0-release-readiness"
BUN_JSC_ADAPTER_SOURCE_REVISION="d82396603b54ba683b82368ce42030d006edbd00"
BUN_JSC_ADAPTER_PROOF_TARGET="check-bun-embed-shared"
BUN_JSC_ADAPTER_SIMDUTF_NAMESPACE="nimbus_bun_simdutf"

BUN_JSC_ADAPTER_REQUIRED_EXPORTS=(
  nimbus_bun_embed_probe_construct_and_destroy_vm
  nimbus_bun_embed_probe_sync_host_call
  nimbus_bun_embed_probe_async_host_call
  nimbus_bun_embed_probe_program_bundle_host_calls
  nimbus_bun_embed_probe_timeout_and_cancel
  nimbus_bun_embed_probe_permission_surface_inventory
  nimbus_bun_embed_probe_memory_behavior
  nimbus_bun_embed_probe_package_module_policy
  nimbus_bun_embed_probe_lifecycle_reuse_stress
  nimbus_bun_embed_invoke_program_wrapper_json
  nimbus_bun_embed_invoke_program_wrapper_json_with_host_bridge
)

bun_jsc_adapter_host_triple() {
  rustc -vV | awk '/^host:/ { print $2 }'
}

bun_jsc_adapter_platform_for_triple() {
  case "$1" in
    *-apple-darwin)
      printf 'darwin\n'
      ;;
    *-unknown-linux-gnu)
      printf 'linux\n'
      ;;
    *-pc-windows-msvc)
      printf 'windows\n'
      ;;
    *)
      printf 'unsupported\n'
      ;;
  esac
}

bun_jsc_adapter_archive_platform_arch_for_triple() {
  case "$1" in
    x86_64-unknown-linux-gnu)
      printf 'linux-x86_64\n'
      ;;
    aarch64-unknown-linux-gnu)
      printf 'linux-arm64\n'
      ;;
    aarch64-apple-darwin)
      printf 'darwin-arm64\n'
      ;;
    x86_64-apple-darwin)
      printf 'darwin-x86_64\n'
      ;;
    *)
      printf 'unsupported-%s\n' "$1"
      ;;
  esac
}

bun_jsc_adapter_library_basename_for_triple() {
  case "$1" in
    *-apple-darwin)
      printf 'libnimbus_bun_jsc_embedder.dylib\n'
      ;;
    *)
      printf 'libnimbus_bun_jsc_embedder.so\n'
      ;;
  esac
}

bun_jsc_adapter_sha256_file() {
  local file_path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${file_path}" | awk '{ print $1 }'
    return 0
  fi
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${file_path}" | awk '{ print $1 }'
    return 0
  fi
  printf 'neither sha256sum nor shasum is available\n' >&2
  return 1
}
