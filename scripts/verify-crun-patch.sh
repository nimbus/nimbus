#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <path-to-crun-source>" >&2
  exit 64
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
patch_file="${repo_root}/patches/crun/0001-krun-add-tsi-port-mapping-via-oci-annotation.patch"
crun_source="$1"
crun_handler="${crun_source}/src/libcrun/handlers/krun.c"

if [[ ! -f "${patch_file}" ]]; then
  echo "patch file not found: ${patch_file}" >&2
  exit 66
fi

if [[ ! -f "${crun_handler}" ]]; then
  echo "expected upstream crun source at: ${crun_handler}" >&2
  exit 66
fi

patch --dry-run -d "${crun_source}" -p1 < "${patch_file}"
echo "verified: ${patch_file} applies cleanly to ${crun_source}"
