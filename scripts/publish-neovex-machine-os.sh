#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: publish-neovex-machine-os.sh [options]

Push a packaged Neovex machine raw-disk OCI layout to a registry and stage a
release bundle if requested.

Options:
  --layout-dir <path>              OCI image-layout directory to publish
  --layout-summary <path>          Explicit OCI layout summary file
  --build-output-dir <path>        Optional build output dir for release staging
  --image-reference <ref>          Destination image reference
  --additional-reference <ref>     Additional tag/reference to push (repeatable)
  --release-dir <path>             Optional directory for staged release assets
  --layout-ref-name <name>         OCI layout ref name override
  -h, --help                       Show this help

Environment:
  NEOVEX_MACHINE_OS_REGISTRY_USERNAME  Optional registry username
  NEOVEX_MACHINE_OS_REGISTRY_PASSWORD  Optional registry password

Example:
  bash scripts/publish-neovex-machine-os.sh \
    --layout-dir /tmp/neovex-machine-os/oci-layout \
    --image-reference docker://ghcr.io/agentstation/neovex-machine-os:v0.1.0 \
    --additional-reference docker://ghcr.io/agentstation/neovex-machine-os:stable \
    --release-dir /tmp/neovex-machine-os/release
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

layout_dir=""
layout_summary=""
build_output_dir=""
image_reference=""
layout_ref_name=""
release_dir=""
declare -a additional_references=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --layout-dir)
      layout_dir="${2:-}"
      shift 2
      ;;
    --layout-summary)
      layout_summary="${2:-}"
      shift 2
      ;;
    --build-output-dir)
      build_output_dir="${2:-}"
      shift 2
      ;;
    --image-reference)
      image_reference="${2:-}"
      shift 2
      ;;
    --additional-reference)
      additional_references+=("${2:-}")
      shift 2
      ;;
    --release-dir)
      release_dir="${2:-}"
      shift 2
      ;;
    --layout-ref-name)
      layout_ref_name="${2:-}"
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

require_command skopeo
require_command sha256sum

if [[ -z "${image_reference}" ]]; then
  echo "--image-reference is required" >&2
  exit 64
fi
if [[ -z "${layout_summary}" && -n "${layout_dir}" ]]; then
  layout_summary="${layout_dir%/}/summary.txt"
fi
if [[ -z "${layout_summary}" ]]; then
  echo "--layout-summary or --layout-dir is required" >&2
  exit 64
fi
if [[ ! -f "${layout_summary}" ]]; then
  echo "layout summary file not found: ${layout_summary}" >&2
  exit 66
fi
if [[ -z "${layout_dir}" ]]; then
  layout_dir="$(summary_value "${layout_summary}" layout_dir)"
fi
if [[ ! -d "${layout_dir}" ]]; then
  echo "OCI layout directory not found: ${layout_dir}" >&2
  exit 66
fi
if [[ -z "${layout_ref_name}" ]]; then
  layout_ref_name="$(summary_value "${layout_summary}" ref_name)"
fi
if [[ -z "${layout_ref_name}" ]]; then
  echo "unable to determine OCI layout ref name from ${layout_summary}" >&2
  exit 65
fi

copy_args=(copy --all --retry-times 3)
if [[ -n "${NEOVEX_MACHINE_OS_REGISTRY_USERNAME:-}" || -n "${NEOVEX_MACHINE_OS_REGISTRY_PASSWORD:-}" ]]; then
  if [[ -z "${NEOVEX_MACHINE_OS_REGISTRY_USERNAME:-}" || -z "${NEOVEX_MACHINE_OS_REGISTRY_PASSWORD:-}" ]]; then
    echo "both NEOVEX_MACHINE_OS_REGISTRY_USERNAME and NEOVEX_MACHINE_OS_REGISTRY_PASSWORD are required together" >&2
    exit 64
  fi
  copy_args+=(--dest-creds "${NEOVEX_MACHINE_OS_REGISTRY_USERNAME}:${NEOVEX_MACHINE_OS_REGISTRY_PASSWORD}")
fi

source_ref="oci:${layout_dir}:${layout_ref_name}"
refs_to_publish=("${image_reference}" "${additional_references[@]}")

for reference in "${refs_to_publish[@]}"; do
  destination_ref="docker://${reference#docker://}"
  skopeo "${copy_args[@]}" "${source_ref}" "${destination_ref}"
done

if [[ -n "${release_dir}" ]]; then
  mkdir -p "${release_dir}"

  if [[ -n "${build_output_dir}" && -f "${build_output_dir%/}/summary.txt" ]]; then
    cp "${build_output_dir%/}/summary.txt" "${release_dir}/build-summary.txt"
    raw_disk_path="$(summary_value "${build_output_dir%/}/summary.txt" compressed_raw_disk_path)"
    if [[ -z "${raw_disk_path}" || "${raw_disk_path}" == "<not-built>" ]]; then
      raw_disk_path="$(summary_value "${build_output_dir%/}/summary.txt" raw_disk_path)"
    fi
    if [[ -n "${raw_disk_path}" && "${raw_disk_path}" != "<not-built>" && -f "${raw_disk_path}" ]]; then
      cp "${raw_disk_path}" "${release_dir}/$(basename "${raw_disk_path}")"
    fi
  fi

  cp "${layout_summary}" "${release_dir}/oci-layout-summary.txt"
  (
    cd "${release_dir}"
    sha256sum ./* >checksums.txt
  )
fi

publish_summary="${release_dir:-${layout_dir}}/publish-summary.txt"
{
  printf 'layout_dir=%s\n' "${layout_dir}"
  printf 'layout_ref_name=%s\n' "${layout_ref_name}"
  printf 'image_reference=%s\n' "${image_reference}"
  if [[ ${#additional_references[@]} -gt 0 ]]; then
    printf 'additional_references=%s\n' "$(IFS=,; printf '%s' "${additional_references[*]}")"
  else
    printf 'additional_references=<none>\n'
  fi
  printf 'release_dir=%s\n' "${release_dir:-<not-staged>}"
} >"${publish_summary}"

printf 'published machine OCI layout from %s\n' "${layout_dir}"
