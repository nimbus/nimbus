#!/usr/bin/env bash
set -euo pipefail

DEFAULT_REPO_URL="https://github.com/coreos/custom-coreos-disk-images.git"
DEFAULT_COMMIT="e017ddda3b20b09627f90f68ef1b708016d10864"

usage() {
  cat <<'EOF'
usage: resolve-custom-coreos-disk-images.sh [options]

Resolve the pinned upstream custom-coreos-disk-images helper that Neovex uses
to turn the built Fedora CoreOS guest image into a bootable raw disk.

Options:
  --helper-path <path>      Use an explicit helper path and print it back
  --checkout-dir <path>     Clone/update the pinned upstream helper here
  --repo-url <url>          Override the upstream repository URL
  --commit <sha>            Override the pinned upstream commit
  -h, --help                Show this help
EOF
}

require_command() {
  local command_name="$1"
  if ! command -v "${command_name}" >/dev/null 2>&1; then
    echo "required command not found: ${command_name}" >&2
    exit 69
  fi
}

helper_path=""
checkout_dir=""
repo_url="${DEFAULT_REPO_URL}"
commit="${DEFAULT_COMMIT}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --helper-path)
      helper_path="${2:-}"
      shift 2
      ;;
    --checkout-dir)
      checkout_dir="${2:-}"
      shift 2
      ;;
    --repo-url)
      repo_url="${2:-}"
      shift 2
      ;;
    --commit)
      commit="${2:-}"
      shift 2
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

if [[ -n "${helper_path}" ]]; then
  if [[ ! -x "${helper_path}" ]]; then
    echo "custom-coreos-disk-images helper is not executable at ${helper_path}" >&2
    exit 66
  fi
  printf '%s\n' "${helper_path}"
  exit 0
fi

if [[ -z "${checkout_dir}" ]]; then
  echo "either --helper-path or --checkout-dir is required" >&2
  exit 64
fi

require_command git

if [[ ! -d "${checkout_dir}/.git" ]]; then
  rm -rf "${checkout_dir}"
  git clone "${repo_url}" "${checkout_dir}" >/dev/null 2>&1
fi

git -C "${checkout_dir}" fetch --depth 1 origin "${commit}" >/dev/null 2>&1
git -C "${checkout_dir}" checkout --detach "${commit}" >/dev/null 2>&1

resolved_helper="${checkout_dir}/custom-coreos-disk-images.sh"
if [[ ! -x "${resolved_helper}" ]]; then
  echo "resolved helper is not executable at ${resolved_helper}" >&2
  exit 66
fi

printf '%s\n' "${resolved_helper}"
