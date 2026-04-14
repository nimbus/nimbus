#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temp_dir="$(mktemp -d)"
trap 'rm -rf "${temp_dir}"' EXIT

raw_disk_path="${temp_dir}/neovex-machine-os.raw.gz"
printf 'raw-disk-bytes-for-layout-test' | gzip -c >"${raw_disk_path}"

layout_dir="${temp_dir}/oci-layout"
bash "${repo_root}/scripts/package-neovex-machine-os-oci.sh" \
  --raw-disk "${raw_disk_path}" \
  --image-reference docker://ghcr.io/agentstation/neovex-machine-os:v1.2.3 \
  --layout-dir "${layout_dir}" \
  --arch arm64

test -f "${layout_dir}/oci-layout"
test -f "${layout_dir}/index.json"
test -f "${layout_dir}/summary.txt"
grep -F '"disktype":"raw"' "${layout_dir}/index.json" >/dev/null
grep -F '"org.opencontainers.image.ref.name":"v1.2.3"' "${layout_dir}/index.json" >/dev/null
grep -F 'layer_media_type=application/vnd.neovex.machine.disk.layer.v1.raw+gzip' "${layout_dir}/summary.txt" >/dev/null
grep -F 'oci_arch=arm64' "${layout_dir}/summary.txt" >/dev/null
grep -F 'image_reference=docker://ghcr.io/agentstation/neovex-machine-os:v1.2.3' "${layout_dir}/summary.txt" >/dev/null

printf 'verified neovex machine-os OCI layout packaging\n'
