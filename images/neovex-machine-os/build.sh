#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: build.sh --neovex-binary <path> [options]

Build the Neovex Fedora CoreOS guest image recipe on Linux.

Options:
  --neovex-binary <path>                 Linux neovex binary to install into the guest
  --output-dir <path>                   Output directory (default: ./out)
  --image-name <reference>              Local OCI tag (default: localhost/neovex-machine-os:dev)
  --fcos-base-image <reference>         Fedora CoreOS base image
  --context-dir <path>                  Reuse a specific staging context instead of mktemp
  --custom-coreos-disk-images <path>    Optional custom-coreos-disk-images.sh path for raw disk output
  --help                                Show this help
EOF
}

require_linux_root() {
  local os_name="${NEOVEX_MACHINE_OS_BUILD_TEST_UNAME:-$(uname -s)}"
  local uid_value="${NEOVEX_MACHINE_OS_BUILD_TEST_UID:-$(id -u)}"
  if [[ "${os_name}" != "Linux" ]]; then
    echo "build.sh must run on Linux" >&2
    exit 1
  fi
  if [[ "${uid_value}" -ne 0 ]]; then
    echo "build.sh must run as root" >&2
    exit 1
  fi
}

require_command() {
  local name="$1"
  if ! command -v "$name" >/dev/null 2>&1; then
    echo "missing required command: $name" >&2
    exit 1
  fi
}

sha256_hex() {
  local target="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${target}" | awk '{print $1}'
    return
  fi
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${target}" | awk '{print $1}'
    return
  fi
  echo "missing required command: sha256sum or shasum" >&2
  exit 1
}

neovex_binary=""
output_dir=""
image_name="localhost/neovex-machine-os:dev"
fcos_base_image="quay.io/fedora/fedora-coreos:stable"
context_dir=""
custom_coreos_disk_images=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --neovex-binary)
      neovex_binary="${2:?missing neovex binary path}"
      shift 2
      ;;
    --output-dir)
      output_dir="${2:?missing output dir}"
      shift 2
      ;;
    --image-name)
      image_name="${2:?missing image name}"
      shift 2
      ;;
    --fcos-base-image)
      fcos_base_image="${2:?missing fcos base image}"
      shift 2
      ;;
    --context-dir)
      context_dir="${2:?missing context dir}"
      shift 2
      ;;
    --custom-coreos-disk-images)
      custom_coreos_disk_images="${2:?missing custom-coreos-disk-images path}"
      shift 2
      ;;
    --help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

require_linux_root
require_command podman

if [[ -z "${neovex_binary}" ]]; then
  echo "--neovex-binary is required" >&2
  usage >&2
  exit 1
fi
if [[ ! -f "${neovex_binary}" ]]; then
  echo "neovex binary does not exist at ${neovex_binary}" >&2
  exit 1
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
output_dir="${output_dir:-${script_dir}/out}"
mkdir -p "${output_dir}"

cleanup_context=0
if [[ -z "${context_dir}" ]]; then
  context_dir="$(mktemp -d)"
  cleanup_context=1
fi

if [[ "${cleanup_context}" -eq 1 ]]; then
  trap 'rm -rf "${context_dir}"' EXIT
fi

install -m 0644 "${script_dir}/Containerfile.COREOS" "${context_dir}/Containerfile.COREOS"
install -m 0755 "${script_dir}/build-common.sh" "${context_dir}/build-common.sh"
install -m 0755 "${neovex_binary}" "${context_dir}/neovex"

podman build \
  -t "${image_name}" \
  -f "${context_dir}/Containerfile.COREOS" \
  "${context_dir}" \
  --build-arg "FCOS_BASE_IMAGE=${fcos_base_image}"

oci_archive_path="${output_dir}/neovex-machine-os.ociarchive"
raw_disk_path=""
compressed_raw_disk_path=""
raw_disk_sha256="<not-built>"
compressed_raw_disk_sha256="<not-built>"

if command -v rpm-ostree >/dev/null 2>&1; then
  rpm-ostree compose build-chunked-oci \
    --bootc \
    --from "${image_name}" \
    --output "oci-archive:${oci_archive_path}"
else
  echo "rpm-ostree not found on host; composing via container"
  rpm_ostree_image="${NEOVEX_RPM_OSTREE_IMAGE:-ghcr.io/agentstation/rpm-ostree:fedora41}"
  podman run --rm --privileged --pull=always \
    --security-opt label=disable \
    --security-opt seccomp=unconfined \
    -v /var/lib/containers/storage:/var/lib/containers/storage \
    -v "${output_dir}:${output_dir}" \
    "${rpm_ostree_image}" \
    bash -c "
      echo '--- runtime diagnostics ---'
      echo \"PATH=\${PATH}\"
      ls -la /usr/bin/osbuild 2>&1 || true
      /usr/bin/osbuild --version 2>&1 || true
      echo '--- end diagnostics ---'
      rpm-ostree compose build-chunked-oci \
        --bootc \
        --from '${image_name}' \
        --output 'oci-archive:${oci_archive_path}'
    "
fi

if [[ -n "${custom_coreos_disk_images}" ]]; then
  if [[ ! -x "${custom_coreos_disk_images}" ]]; then
    echo "custom-coreos-disk-images helper is not executable at ${custom_coreos_disk_images}" >&2
    exit 1
  fi
  (
    cd "${output_dir}"
    bash "${custom_coreos_disk_images}" \
      --platforms applehv \
      --ociarchive "${oci_archive_path}" \
      --osname fedora-coreos \
      --imgref "ostree-unverified-registry:${image_name}" \
      --metal-image-size 6144 \
      --extra-kargs='ostree.prepare-root.composefs=0'
  )
  raw_disk_path="$(find "${output_dir}" -maxdepth 1 -type f -name '*.raw' | head -n 1 || true)"
  if [[ -n "${raw_disk_path}" ]]; then
    require_command gzip
    compressed_raw_disk_path="${output_dir}/neovex-machine-os.raw.gz"
    gzip -c "${raw_disk_path}" >"${compressed_raw_disk_path}"
    raw_disk_sha256="$(sha256_hex "${raw_disk_path}")"
    compressed_raw_disk_sha256="$(sha256_hex "${compressed_raw_disk_path}")"
  fi
fi

neovex_binary_sha256="$(sha256_hex "${neovex_binary}")"
containerfile_sha256="$(sha256_hex "${script_dir}/Containerfile.COREOS")"
build_common_sha256="$(sha256_hex "${script_dir}/build-common.sh")"
oci_archive_sha256="$(sha256_hex "${oci_archive_path}")"

cat >"${output_dir}/summary.txt" <<EOF
image_name=${image_name}
fcos_base_image=${fcos_base_image}
neovex_binary=${neovex_binary}
neovex_binary_sha256=${neovex_binary_sha256}
containerfile_sha256=${containerfile_sha256}
build_common_sha256=${build_common_sha256}
oci_archive_path=${oci_archive_path}
oci_archive_sha256=${oci_archive_sha256}
raw_disk_path=${raw_disk_path:-<not-built>}
raw_disk_sha256=${raw_disk_sha256}
compressed_raw_disk_path=${compressed_raw_disk_path:-<not-built>}
compressed_raw_disk_sha256=${compressed_raw_disk_sha256}
custom_coreos_disk_images=${custom_coreos_disk_images:-<not-run>}
EOF
