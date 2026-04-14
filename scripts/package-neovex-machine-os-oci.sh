#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: package-neovex-machine-os-oci.sh [options]

Wrap a Neovex machine raw-disk artifact in the OCI image-layout shape that the
macOS machine manager already consumes from registries.

Options:
  --build-output-dir <path>    Build output directory containing summary.txt
  --summary-file <path>        Explicit build summary file to read
  --raw-disk <path>            Explicit raw/raw.gz/raw.zst machine disk artifact
  --layout-dir <path>          Output OCI image-layout directory
  --image-reference <ref>      Destination image reference used to derive ref name
  --ref-name <name>            Explicit OCI layout ref name (defaults from image reference)
  --arch <arch>                OCI architecture (default: host architecture)
  --os <os>                    OCI operating system (default: linux)
  -h, --help                   Show this help

Examples:
  bash scripts/package-neovex-machine-os-oci.sh \
    --build-output-dir /tmp/neovex-machine-os \
    --image-reference docker://ghcr.io/agentstation/neovex-machine-os:v0.1.0 \
    --layout-dir /tmp/neovex-machine-os/oci-layout
EOF
}

require_command() {
  local command_name="$1"
  if ! command -v "${command_name}" >/dev/null 2>&1; then
    echo "required command not found: ${command_name}" >&2
    exit 69
  fi
}

summary_value() {
  local summary_file="$1"
  local key="$2"
  awk -F= -v target="${key}" '$1 == target { print substr($0, length($1) + 2) }' "${summary_file}" | tail -n 1
}

normalize_arch() {
  case "$1" in
    aarch64|arm64) printf 'arm64\n' ;;
    x86_64|amd64) printf 'amd64\n' ;;
    *) printf '%s\n' "$1" ;;
  esac
}

infer_layer_media_type() {
  case "$1" in
    *.raw.gz) printf 'application/vnd.neovex.machine.disk.layer.v1.raw+gzip\n' ;;
    *.raw.zst) printf 'application/vnd.neovex.machine.disk.layer.v1.raw+zstd\n' ;;
    *.raw) printf 'application/vnd.neovex.machine.disk.layer.v1.raw\n' ;;
    *) printf 'application/vnd.neovex.machine.disk.layer.v1.blob\n' ;;
  esac
}

derive_ref_name() {
  local reference="$1"
  local stripped="${reference#docker://}"
  if [[ "${stripped}" == *@* ]]; then
    echo "image reference must use a tag, not a digest, when deriving an OCI layout ref name: ${reference}" >&2
    exit 64
  fi
  local last_component="${stripped##*/}"
  if [[ "${last_component}" == *:* ]]; then
    printf '%s\n' "${last_component##*:}"
  else
    printf 'latest\n'
  fi
}

sha256_hex() {
  sha256sum "$1" | awk '{print $1}'
}

file_size_bytes() {
  wc -c <"$1" | tr -d '[:space:]'
}

build_output_dir=""
summary_file=""
raw_disk_path=""
layout_dir=""
image_reference=""
ref_name=""
oci_arch="$(normalize_arch "${NEOVEX_MACHINE_OS_PACKAGE_TEST_ARCH:-$(uname -m)}")"
oci_os="linux"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --build-output-dir)
      build_output_dir="${2:-}"
      shift 2
      ;;
    --summary-file)
      summary_file="${2:-}"
      shift 2
      ;;
    --raw-disk)
      raw_disk_path="${2:-}"
      shift 2
      ;;
    --layout-dir)
      layout_dir="${2:-}"
      shift 2
      ;;
    --image-reference)
      image_reference="${2:-}"
      shift 2
      ;;
    --ref-name)
      ref_name="${2:-}"
      shift 2
      ;;
    --arch)
      oci_arch="$(normalize_arch "${2:-}")"
      shift 2
      ;;
    --os)
      oci_os="${2:-}"
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

require_command sha256sum
require_command mktemp

if [[ -z "${summary_file}" && -n "${build_output_dir}" ]]; then
  summary_file="${build_output_dir%/}/summary.txt"
fi

if [[ -n "${summary_file}" ]]; then
  if [[ ! -f "${summary_file}" ]]; then
    echo "build summary file not found: ${summary_file}" >&2
    exit 66
  fi
  if [[ -z "${raw_disk_path}" ]]; then
    raw_disk_path="$(summary_value "${summary_file}" compressed_raw_disk_path)"
    if [[ -z "${raw_disk_path}" || "${raw_disk_path}" == "<not-built>" ]]; then
      raw_disk_path="$(summary_value "${summary_file}" raw_disk_path)"
    fi
  fi
fi

if [[ -z "${raw_disk_path}" ]]; then
  echo "a raw-disk artifact is required; pass --raw-disk or a summary file with raw_disk_path/compressed_raw_disk_path" >&2
  exit 64
fi
if [[ ! -f "${raw_disk_path}" ]]; then
  echo "raw-disk artifact not found: ${raw_disk_path}" >&2
  exit 66
fi

if [[ -z "${image_reference}" && -z "${ref_name}" ]]; then
  echo "either --image-reference or --ref-name is required" >&2
  exit 64
fi
if [[ -z "${ref_name}" ]]; then
  ref_name="$(derive_ref_name "${image_reference}")"
fi
if [[ -z "${layout_dir}" ]]; then
  base_dir="${build_output_dir:-$(dirname "${raw_disk_path}")}"
  layout_dir="${base_dir%/}/oci-layout"
fi

rm -rf "${layout_dir}"
mkdir -p "${layout_dir}/blobs/sha256"

layer_title="$(basename "${raw_disk_path}")"
layer_media_type="$(infer_layer_media_type "${raw_disk_path}")"
layer_size="$(file_size_bytes "${raw_disk_path}")"
layer_hex="$(sha256_hex "${raw_disk_path}")"
layer_digest="sha256:${layer_hex}"
layer_blob_path="${layout_dir}/blobs/sha256/${layer_hex}"
cp "${raw_disk_path}" "${layer_blob_path}"

temp_dir="$(mktemp -d)"
trap 'rm -rf "${temp_dir}"' EXIT

config_path="${temp_dir}/config.json"
cat >"${config_path}" <<EOF
{"architecture":"${oci_arch}","os":"${oci_os}","rootfs":{"type":"layers","diff_ids":[]},"config":{}}
EOF
config_size="$(file_size_bytes "${config_path}")"
config_hex="$(sha256_hex "${config_path}")"
config_digest="sha256:${config_hex}"
cp "${config_path}" "${layout_dir}/blobs/sha256/${config_hex}"

manifest_path="${temp_dir}/manifest.json"
cat >"${manifest_path}" <<EOF
{"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","config":{"mediaType":"application/vnd.oci.image.config.v1+json","size":${config_size},"digest":"${config_digest}"},"layers":[{"mediaType":"${layer_media_type}","size":${layer_size},"digest":"${layer_digest}","annotations":{"org.opencontainers.image.title":"${layer_title}"}}]}
EOF
manifest_size="$(file_size_bytes "${manifest_path}")"
manifest_hex="$(sha256_hex "${manifest_path}")"
manifest_digest="sha256:${manifest_hex}"
cp "${manifest_path}" "${layout_dir}/blobs/sha256/${manifest_hex}"

cat >"${layout_dir}/index.json" <<EOF
{"schemaVersion":2,"mediaType":"application/vnd.oci.image.index.v1+json","manifests":[{"mediaType":"application/vnd.oci.image.manifest.v1+json","size":${manifest_size},"digest":"${manifest_digest}","platform":{"architecture":"${oci_arch}","os":"${oci_os}"},"annotations":{"disktype":"raw","org.opencontainers.image.ref.name":"${ref_name}"}}]}
EOF
printf '{"imageLayoutVersion":"1.0.0"}\n' >"${layout_dir}/oci-layout"

summary_output="${layout_dir}/summary.txt"
cat >"${summary_output}" <<EOF
raw_disk_path=${raw_disk_path}
image_reference=${image_reference:-<unspecified>}
ref_name=${ref_name}
oci_arch=${oci_arch}
oci_os=${oci_os}
layer_media_type=${layer_media_type}
layer_title=${layer_title}
layer_digest=${layer_digest}
manifest_digest=${manifest_digest}
layout_dir=${layout_dir}
EOF

printf 'packaged machine OCI layout at %s\n' "${layout_dir}"
