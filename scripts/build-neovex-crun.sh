#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: build-neovex-crun.sh --source <path> [--output <path>] [--install-path <path>] [--sudo-install] [--build-dir <path>]

Build the checked-in patched neovex crun binary from a pinned upstream crun
source checkout. This helper is Linux-only and preserves the original source
checkout by building from a copied worktree.

examples:
  bash scripts/build-neovex-crun.sh \
    --source ~/src/github.com/containers/crun \
    --output /tmp/neovex-crun-stage/crun

  bash scripts/build-neovex-crun.sh \
    --source ~/src/github.com/containers/crun \
    --output /tmp/neovex-crun-stage/crun \
    --install-path /usr/libexec/neovex/crun \
    --sudo-install
EOF
}

require_command() {
  local command_name="$1"

  if ! command -v "${command_name}" >/dev/null 2>&1; then
    echo "required command not found: ${command_name}" >&2
    exit 69
  fi
}

source_dir=""
output_path="${TMPDIR:-/tmp}/neovex-crun-stage/crun"
install_path=""
build_dir=""
sudo_install=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --source)
      source_dir="${2:-}"
      shift 2
      ;;
    --output)
      output_path="${2:-}"
      shift 2
      ;;
    --install-path)
      install_path="${2:-}"
      shift 2
      ;;
    --build-dir)
      build_dir="${2:-}"
      shift 2
      ;;
    --sudo-install)
      sudo_install=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 64
      ;;
  esac
done

if [[ -z "${source_dir}" ]]; then
  usage >&2
  exit 64
fi

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "build-neovex-crun.sh requires a Linux host" >&2
  exit 69
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
patch_file="${repo_root}/patches/crun/0001-krun-add-tsi-port-mapping-via-oci-annotation.patch"

if [[ ! -d "${source_dir}" ]]; then
  echo "upstream crun source not found: ${source_dir}" >&2
  exit 66
fi

if [[ ! -f "${patch_file}" ]]; then
  echo "patch file not found: ${patch_file}" >&2
  exit 66
fi

require_command bash
require_command patch
require_command make
require_command autoreconf
require_command autoconf
require_command automake
require_command pkg-config
require_command cp
require_command install

if ! command -v cc >/dev/null 2>&1 && ! command -v gcc >/dev/null 2>&1 && ! command -v clang >/dev/null 2>&1; then
  echo "required C compiler not found (cc, gcc, or clang)" >&2
  exit 69
fi

bash "${repo_root}/scripts/verify-crun-patch.sh" "${source_dir}"

cleanup_build_dir=0
if [[ -z "${build_dir}" ]]; then
  build_dir="$(mktemp -d "${TMPDIR:-/tmp}/neovex-crun-build.XXXXXX")"
  cleanup_build_dir=1
fi

mkdir -p "${build_dir}"
if find "${build_dir}" -mindepth 1 -maxdepth 1 | read -r _; then
  echo "build directory must be empty: ${build_dir}" >&2
  exit 64
fi

cleanup() {
  if [[ "${cleanup_build_dir}" -eq 1 ]]; then
    rm -rf "${build_dir}"
  fi
}
trap cleanup EXIT

cp -R "${source_dir}/." "${build_dir}"

echo "build.source=${source_dir}"
echo "build.dir=${build_dir}"
echo "build.patch=${patch_file}"

patch -d "${build_dir}" -p1 < "${patch_file}"

(
  cd "${build_dir}"
  ./autogen.sh
  ./configure --with-libkrun
  # Build the generated libocispec and git-version prerequisites explicitly.
  # This matches upstream's packaged build flow and avoids fresh-worktree
  # races around generated headers.
  make -j1 -C libocispec libocispec.la
  make -j1 git-version.h
  make -j1 crun
)

built_binary="${build_dir}/crun"
if [[ ! -x "${built_binary}" ]]; then
  echo "expected built binary not found: ${built_binary}" >&2
  exit 70
fi

mkdir -p "$(dirname "${output_path}")"
install -m 0755 "${built_binary}" "${output_path}"
echo "build.output=${output_path}"

version_line="$("${output_path}" --version 2>/dev/null | head -n1 || true)"
if [[ -n "${version_line}" ]]; then
  echo "build.output.version=${version_line}"
fi

if [[ -n "${install_path}" ]]; then
  if [[ "${sudo_install}" -eq 1 ]]; then
    sudo mkdir -p "$(dirname "${install_path}")"
    sudo install -m 0755 "${output_path}" "${install_path}"
  else
    mkdir -p "$(dirname "${install_path}")"
    install -m 0755 "${output_path}" "${install_path}"
  fi
  echo "build.install=${install_path}"

  install_version_line="$("${install_path}" --version 2>/dev/null | head -n1 || true)"
  if [[ -n "${install_version_line}" ]]; then
    echo "build.install.version=${install_version_line}"
  fi
fi
