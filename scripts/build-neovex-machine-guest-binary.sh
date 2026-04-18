#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root_override="${NEOVEX_MACHINE_GUEST_BUILD_REPO_ROOT:-}"
if [[ -n "${repo_root_override}" ]]; then
  repo_root="$(cd "${repo_root_override}" && pwd)"
else
  repo_root="$(cd "${script_dir}/.." && pwd)"
fi

usage() {
  cat <<'EOF'
usage: build-neovex-machine-guest-binary.sh [options]

Build the Linux guest `neovex` binary that macOS developer-machine runs stage
into the guest before validating the forwarded machine API.

By default the helper chooses the Linux target triple that matches the local
host CPU architecture and leaves the binary at:

  target/<triple>/<profile>/neovex

That is the same path the checked-in macOS machine manager now prefers before
falling back to release downloads, so a successful helper run becomes the
automatic local guest-binary source for `neovex machine start`.

options:
  --target <triple>            Explicit Linux target triple override
  --profile <release|debug>    Cargo profile to build (default: release)
  --copy-to <path>             Copy the built binary to an explicit path and
                               print that copied path instead of the target dir
  --cache-root <path>          Cache root for cargo-zigbuild + zig
                               (default: ${TMPDIR:-/tmp}/neovex-machine-guest-build)
  --cargo <path>               Cargo command/path (default: cargo)
  --rustup <path>              Rustup command/path (default: rustup)
  --zig <path>                 Zig command/path (default: zig)
  -h, --help                   Show this help

examples:
  bash scripts/build-neovex-machine-guest-binary.sh
  bash scripts/build-neovex-machine-guest-binary.sh --copy-to /tmp/neovex-linux-guest/neovex
EOF
}

log() {
  printf '%s\n' "$*" >&2
}

fail() {
  log "$*"
  exit 64
}

require_command() {
  local command_name="$1"
  if ! command -v "${command_name}" >/dev/null 2>&1; then
    fail "required command not found: ${command_name}"
  fi
}

default_linux_target_for_host() {
  local host_os
  local host_arch
  host_os="$(uname -s)"
  host_arch="$(uname -m)"

  case "${host_os}:${host_arch}" in
    Darwin:arm64|Darwin:aarch64|Linux:arm64|Linux:aarch64)
      printf 'aarch64-unknown-linux-gnu\n'
      ;;
    Darwin:x86_64|Linux:x86_64)
      printf 'x86_64-unknown-linux-gnu\n'
      ;;
    *)
      fail "unsupported host platform for guest binary build helper: ${host_os}/${host_arch}"
      ;;
  esac
}

native_linux_target_for_host() {
  local host_os
  host_os="$(uname -s)"
  if [[ "${host_os}" != "Linux" ]]; then
    return 1
  fi
  default_linux_target_for_host
}

ensure_cargo_zigbuild() {
  local cargo_command="$1"
  local install_root="$2"
  local cargo_zigbuild_path="${install_root}/bin/cargo-zigbuild"

  if [[ ! -x "${cargo_zigbuild_path}" ]]; then
    log "installing cargo-zigbuild into ${install_root}"
    "${cargo_command}" install --root "${install_root}" cargo-zigbuild >&2
  fi

  export PATH="${install_root}/bin:${PATH}"
}

configure_zigbuild_environment() {
  local target_triple="$1"
  local cache_root="$2"
  local host_os
  local target_suffix
  local ar_var
  local ranlib_var

  mkdir -p "${cache_root}"

  export CARGO_ZIGBUILD_CACHE_DIR="${cache_root}/cargo-zigbuild-cache"
  export ZIG_LOCAL_CACHE_DIR="${cache_root}/zig-local-cache"
  export ZIG_GLOBAL_CACHE_DIR="${cache_root}/zig-global-cache"
  export LIBZ_SYS_STATIC=1

  mkdir -p \
    "${CARGO_ZIGBUILD_CACHE_DIR}" \
    "${ZIG_LOCAL_CACHE_DIR}" \
    "${ZIG_GLOBAL_CACHE_DIR}"

  host_os="$(uname -s)"
  if [[ "${host_os}" == "Darwin" ]]; then
    target_suffix="${target_triple//-/_}"
    printf -v ar_var 'AR_%s' "${target_suffix}"
    printf -v ranlib_var 'RANLIB_%s' "${target_suffix}"
    printf -v "${ar_var}" '%s' "/usr/bin/ar"
    printf -v "${ranlib_var}" '%s' "/usr/bin/ranlib"
    export "${ar_var}" "${ranlib_var}"
  fi
}

target_triple=""
profile="release"
copy_to=""
cache_root="${NEOVEX_MACHINE_GUEST_BUILD_CACHE_ROOT:-${TMPDIR:-/tmp}/neovex-machine-guest-build}"
cargo_command="${CARGO:-cargo}"
rustup_command="${RUSTUP:-rustup}"
zig_command="${ZIG:-zig}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target)
      target_triple="${2:?missing target triple}"
      shift 2
      ;;
    --profile)
      profile="${2:?missing profile}"
      shift 2
      ;;
    --copy-to)
      copy_to="${2:?missing copy path}"
      shift 2
      ;;
    --cache-root)
      cache_root="${2:?missing cache root}"
      shift 2
      ;;
    --cargo)
      cargo_command="${2:?missing cargo path}"
      shift 2
      ;;
    --rustup)
      rustup_command="${2:?missing rustup path}"
      shift 2
      ;;
    --zig)
      zig_command="${2:?missing zig path}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
done

case "${profile}" in
  release|debug)
    ;;
  *)
    fail "unsupported profile '${profile}'; expected release or debug"
    ;;
esac

target_triple="${target_triple:-$(default_linux_target_for_host)}"

require_command "${cargo_command}"
require_command "${rustup_command}"

build_subcommand="build"
native_target=""
if native_target="$(native_linux_target_for_host 2>/dev/null)" && [[ "${target_triple}" == "${native_target}" ]]; then
  log "building native Linux guest binary for ${target_triple}"
else
  require_command "${zig_command}"
  ensure_cargo_zigbuild "${cargo_command}" "${cache_root}/cargo-zigbuild"
  configure_zigbuild_environment "${target_triple}" "${cache_root}"
  build_subcommand="zigbuild"
  log "building cross Linux guest binary for ${target_triple} via cargo-zigbuild"
fi

log "ensuring Rust target ${target_triple}"
"${rustup_command}" target add "${target_triple}" >&2

build_cmd=(
  "${cargo_command}"
  "${build_subcommand}"
  --target
  "${target_triple}"
  -p
  neovex-bin
)

if [[ "${profile}" == "release" ]]; then
  build_cmd+=( --release )
fi

(cd "${repo_root}" && "${build_cmd[@]}" >&2)

built_binary="${repo_root}/target/${target_triple}/${profile}/neovex"
if [[ ! -x "${built_binary}" ]]; then
  fail "expected built guest binary at ${built_binary}"
fi

if [[ -n "${copy_to}" ]]; then
  mkdir -p "$(dirname "${copy_to}")"
  install -m 0755 "${built_binary}" "${copy_to}"
  printf '%s\n' "${copy_to}"
  exit 0
fi

printf '%s\n' "${built_binary}"
