#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: build-neovex-machine-os.sh [options]

Build the Neovex macOS guest image recipe on a Linux host using the checked-in
image recipe and a Linux `neovex` binary.

Options:
  --neovex-binary <path>              Existing Linux neovex binary to install into the guest
  --cargo-profile <profile>           Cargo profile to build when --neovex-binary is omitted (default: release)
  --output-dir <path>                 Output directory passed through to the image recipe
  --image-name <reference>            OCI tag passed through to the image recipe
  --fcos-base-image <reference>       Fedora CoreOS base image passed through to the image recipe
  --context-dir <path>                Reused staging context passed through to the image recipe
  --custom-coreos-disk-images <path>  Optional raw-disk helper passed through to the image recipe
  --fetch-custom-coreos-disk-images <dir>
                                      Clone/update the pinned upstream helper into this checkout dir
  -h, --help                          Show this help

Examples:
  sudo bash scripts/build-neovex-machine-os.sh \
    --neovex-binary /absolute/path/to/neovex-linux-aarch64 \
    --output-dir /tmp/neovex-machine-os

  sudo bash scripts/build-neovex-machine-os.sh \
    --cargo-profile release \
    --output-dir /tmp/neovex-machine-os \
    --fetch-custom-coreos-disk-images /tmp/neovex-machine-os-helper
EOF
}

require_command() {
  local command_name="$1"
  if ! command -v "${command_name}" >/dev/null 2>&1; then
    echo "required command not found: ${command_name}" >&2
    exit 69
  fi
}

neovex_binary=""
cargo_profile="release"
output_dir=""
image_name=""
fcos_base_image=""
context_dir=""
custom_coreos_disk_images=""
fetch_custom_coreos_disk_images_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --neovex-binary)
      neovex_binary="${2:-}"
      shift 2
      ;;
    --cargo-profile)
      cargo_profile="${2:-}"
      shift 2
      ;;
    --output-dir)
      output_dir="${2:-}"
      shift 2
      ;;
    --image-name)
      image_name="${2:-}"
      shift 2
      ;;
    --fcos-base-image)
      fcos_base_image="${2:-}"
      shift 2
      ;;
    --context-dir)
      context_dir="${2:-}"
      shift 2
      ;;
    --custom-coreos-disk-images)
      custom_coreos_disk_images="${2:-}"
      shift 2
      ;;
    --fetch-custom-coreos-disk-images)
      fetch_custom_coreos_disk_images_dir="${2:-}"
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

os_name="${NEOVEX_MACHINE_OS_BUILD_WRAPPER_TEST_UNAME:-$(uname -s)}"
if [[ "${os_name}" != "Linux" ]]; then
  echo "build-neovex-machine-os.sh requires a Linux host" >&2
  exit 69
fi

require_command bash

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
recipe_script="${repo_root}/images/neovex-machine-os/build.sh"
resolve_helper_script="${repo_root}/scripts/resolve-custom-coreos-disk-images.sh"

if [[ ! -f "${recipe_script}" ]]; then
  echo "image recipe entrypoint not found: ${recipe_script}" >&2
  exit 66
fi
if [[ ! -f "${resolve_helper_script}" ]]; then
  echo "custom-coreos-disk-images resolver not found: ${resolve_helper_script}" >&2
  exit 66
fi
if [[ -n "${custom_coreos_disk_images}" && -n "${fetch_custom_coreos_disk_images_dir}" ]]; then
  echo "pass either --custom-coreos-disk-images or --fetch-custom-coreos-disk-images, not both" >&2
  exit 64
fi

if [[ -z "${neovex_binary}" ]]; then
  require_command cargo
  case "${cargo_profile}" in
    release)
      (
        cd "${repo_root}"
        cargo build --release -p neovex-bin
      )
      neovex_binary="${repo_root}/target/release/neovex"
      ;;
    dev|debug)
      (
        cd "${repo_root}"
        cargo build -p neovex-bin
      )
      neovex_binary="${repo_root}/target/debug/neovex"
      ;;
    *)
      echo "unsupported cargo profile: ${cargo_profile} (expected release|dev|debug)" >&2
      exit 64
      ;;
  esac
fi

if [[ ! -f "${neovex_binary}" ]]; then
  echo "neovex binary not found: ${neovex_binary}" >&2
  exit 66
fi

echo "build.neovex_binary=${neovex_binary}"
echo "build.recipe=${recipe_script}"

if [[ -n "${fetch_custom_coreos_disk_images_dir}" ]]; then
  custom_coreos_disk_images="$(
    bash "${resolve_helper_script}" \
      --checkout-dir "${fetch_custom_coreos_disk_images_dir}"
  )"
  echo "build.custom_coreos_disk_images=${custom_coreos_disk_images}"
fi

args=(--neovex-binary "${neovex_binary}")
if [[ -n "${output_dir}" ]]; then
  args+=(--output-dir "${output_dir}")
fi
if [[ -n "${image_name}" ]]; then
  args+=(--image-name "${image_name}")
fi
if [[ -n "${fcos_base_image}" ]]; then
  args+=(--fcos-base-image "${fcos_base_image}")
fi
if [[ -n "${context_dir}" ]]; then
  args+=(--context-dir "${context_dir}")
fi
if [[ -n "${custom_coreos_disk_images}" ]]; then
  args+=(--custom-coreos-disk-images "${custom_coreos_disk_images}")
fi

bash "${recipe_script}" "${args[@]}"
