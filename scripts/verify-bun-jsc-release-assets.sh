#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-bun-jsc-release-assets.sh --artifacts-dir <path> [options]

Verify optional Bun/JSC adapter release assets. With no adapter archives present
and no --require-platform arguments, this exits successfully and records that
the optional adapter lane was intentionally absent from the release artifact
set.

Required:
  --artifacts-dir <path>       Directory containing release assets

Optional:
  --checksums <path>           Checksum file that must include every adapter archive
  --require-platform <name>    Require an adapter archive, e.g. linux-x86_64 or darwin-arm64
  --nm <path>                  nm-compatible command for package verifier test hooks
  -h, --help                   Show this help text
EOF
}

die() {
  printf '%s\n' "$*" >&2
  exit 64
}

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
source "${repo_root}/scripts/bun-jsc-adapter-contract.sh"

artifacts_dir=""
checksums_path=""
nm_bin=""
required_platforms=()

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --artifacts-dir)
      artifacts_dir="${2:-}"
      shift 2
      ;;
    --checksums)
      checksums_path="${2:-}"
      shift 2
      ;;
    --require-platform)
      required_platforms+=("${2:-}")
      shift 2
      ;;
    --nm)
      nm_bin="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[[ -n "${artifacts_dir}" ]] || die "--artifacts-dir is required"
[[ -d "${artifacts_dir}" ]] || die "artifacts dir does not exist: ${artifacts_dir}"
if [[ -n "${checksums_path}" && ! -f "${checksums_path}" ]]; then
  die "checksums file does not exist: ${checksums_path}"
fi

platform_to_target_triple() {
  case "$1" in
    linux-x86_64)
      printf 'x86_64-unknown-linux-gnu\n'
      ;;
    linux-arm64)
      printf 'aarch64-unknown-linux-gnu\n'
      ;;
    darwin-arm64)
      printf 'aarch64-apple-darwin\n'
      ;;
    darwin-x86_64)
      printf 'x86_64-apple-darwin\n'
      ;;
    *)
      return 1
      ;;
  esac
}

artifacts_dir="$(cd "${artifacts_dir}" && pwd)"
shopt -s nullglob
adapter_archives=("${artifacts_dir}"/nimbus-bun-jsc-adapter-*.tar.gz)
shopt -u nullglob

if [[ "${#required_platforms[@]}" -gt 0 ]]; then
  for platform in "${required_platforms[@]}"; do
    [[ -n "${platform}" ]] || die "--require-platform must be non-empty"
    if ! platform_to_target_triple "${platform}" >/dev/null; then
      die "unsupported required Bun/JSC adapter platform: ${platform}"
    fi
    expected="${artifacts_dir}/nimbus-bun-jsc-adapter-${platform}.tar.gz"
    [[ -f "${expected}" ]] || die "required Bun/JSC adapter archive is missing: ${expected}"
  done
fi

if [[ "${#adapter_archives[@]}" -eq 0 ]]; then
  if [[ "${#required_platforms[@]}" -eq 0 ]]; then
    printf 'verified: optional Bun/JSC adapter release assets are absent by policy\n'
    exit 0
  fi
  die "required Bun/JSC adapter archives were not found"
fi

for archive in "${adapter_archives[@]}"; do
  basename="$(basename "${archive}")"
  platform="${basename#nimbus-bun-jsc-adapter-}"
  platform="${platform%.tar.gz}"
  target_triple="$(platform_to_target_triple "${platform}" || true)"
  [[ -n "${target_triple}" ]] ||
    die "unsupported Bun/JSC adapter release asset platform: ${basename}"

  verify_args=(
    bash "${repo_root}/scripts/verify-bun-jsc-adapter-package.sh"
    --archive "${archive}"
    --target-triple "${target_triple}"
  )
  if [[ -n "${nm_bin}" ]]; then
    verify_args+=(--nm "${nm_bin}")
  fi
  "${verify_args[@]}"

  if [[ -n "${checksums_path}" ]]; then
    digest="$(bun_jsc_adapter_sha256_file "${archive}")"
    grep -F "${digest}  ${basename}" "${checksums_path}" >/dev/null ||
      die "checksums file does not contain matching ${basename} digest"
  fi
done

printf 'verified: optional Bun/JSC adapter release assets match package and checksum contracts (%s archive(s))\n' "${#adapter_archives[@]}"
