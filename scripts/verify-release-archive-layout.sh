#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-release-archive-layout.sh --artifacts-dir <path>

Verify the published Nimbus release-archive layout before checksums, release
creation, or downstream packaging updates consume it.

Current contract:
- nimbus_darwin_arm64.tar.gz contains:
  - nimbus
  - libexec/gvproxy
  - README.md
  - LICENSE
- nimbus_linux_x86_64.tar.gz and nimbus_linux_arm64.tar.gz contain:
  - nimbus
  - README.md
  - LICENSE
  - no macOS-only libexec/gvproxy helper
- nimbus_windows_x86_64.zip contains:
  - nimbus.exe
  - README.md
  - LICENSE
EOF
}

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

assert_present() {
  local path="$1"
  [[ -e "${path}" ]] || die "expected path missing: ${path}"
}

assert_absent() {
  local path="$1"
  [[ ! -e "${path}" ]] || die "unexpected path present: ${path}"
}

assert_executable() {
  local path="$1"
  [[ -x "${path}" ]] || die "expected executable path missing execute bit: ${path}"
}

artifacts_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --artifacts-dir)
      artifacts_dir="${2:?missing artifacts dir}"
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

command -v tar >/dev/null 2>&1 || die "tar is required"
command -v unzip >/dev/null 2>&1 || die "unzip is required"

artifacts_dir="$(cd "${artifacts_dir}" && pwd)"
tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-release-archives.XXXXXX")"
trap 'rm -rf "${tmp_root}"' EXIT

darwin_archive="${artifacts_dir}/nimbus_darwin_arm64.tar.gz"
linux_x86_archive="${artifacts_dir}/nimbus_linux_x86_64.tar.gz"
linux_arm_archive="${artifacts_dir}/nimbus_linux_arm64.tar.gz"
windows_archive="${artifacts_dir}/nimbus_windows_x86_64.zip"

assert_present "${darwin_archive}"
assert_present "${linux_x86_archive}"
assert_present "${linux_arm_archive}"
assert_present "${windows_archive}"

darwin_dir="${tmp_root}/darwin"
linux_x86_dir="${tmp_root}/linux-x86_64"
linux_arm_dir="${tmp_root}/linux-arm64"
windows_dir="${tmp_root}/windows-x86_64"
mkdir -p "${darwin_dir}" "${linux_x86_dir}" "${linux_arm_dir}" "${windows_dir}"

tar -xzf "${darwin_archive}" -C "${darwin_dir}"
tar -xzf "${linux_x86_archive}" -C "${linux_x86_dir}"
tar -xzf "${linux_arm_archive}" -C "${linux_arm_dir}"
unzip -q "${windows_archive}" -d "${windows_dir}"

assert_present "${darwin_dir}/README.md"
assert_present "${darwin_dir}/LICENSE"
assert_present "${darwin_dir}/nimbus"
assert_executable "${darwin_dir}/nimbus"
assert_present "${darwin_dir}/libexec/gvproxy"
assert_executable "${darwin_dir}/libexec/gvproxy"

assert_present "${linux_x86_dir}/README.md"
assert_present "${linux_x86_dir}/LICENSE"
assert_present "${linux_x86_dir}/nimbus"
assert_executable "${linux_x86_dir}/nimbus"
assert_absent "${linux_x86_dir}/libexec/gvproxy"

assert_present "${linux_arm_dir}/README.md"
assert_present "${linux_arm_dir}/LICENSE"
assert_present "${linux_arm_dir}/nimbus"
assert_executable "${linux_arm_dir}/nimbus"
assert_absent "${linux_arm_dir}/libexec/gvproxy"

assert_present "${windows_dir}/README.md"
assert_present "${windows_dir}/LICENSE"
assert_present "${windows_dir}/nimbus.exe"

printf 'verified: release archives match the published binary/layout contract\n'
