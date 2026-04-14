#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temp_dir="$(mktemp -d)"
trap 'rm -rf "${temp_dir}"' EXIT

raw_disk_path="${temp_dir}/neovex-machine-os.raw.gz"
printf 'raw-disk-bytes-for-publish-test' | gzip -c >"${raw_disk_path}"

layout_dir="${temp_dir}/oci-layout"
bash "${repo_root}/scripts/package-neovex-machine-os-oci.sh" \
  --raw-disk "${raw_disk_path}" \
  --image-reference docker://ghcr.io/agentstation/neovex-machine-os:latest \
  --layout-dir "${layout_dir}" \
  --arch arm64

mkdir -p "${temp_dir}/bin"
cat >"${temp_dir}/bin/skopeo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"${TMPDIR}/skopeo.log"
exit 0
EOF
chmod 0755 "${temp_dir}/bin/skopeo"

release_dir="${temp_dir}/release"
PATH="${temp_dir}/bin:${PATH}" \
TMPDIR="${temp_dir}" \
NEOVEX_MACHINE_OS_REGISTRY_USERNAME=neovex \
NEOVEX_MACHINE_OS_REGISTRY_PASSWORD=secret \
bash "${repo_root}/scripts/publish-neovex-machine-os.sh" \
  --layout-dir "${layout_dir}" \
  --image-reference docker://ghcr.io/agentstation/neovex-machine-os:latest \
  --additional-reference docker://ghcr.io/agentstation/neovex-machine-os:next \
  --release-dir "${release_dir}"

grep -F -- '--dest-creds neovex:secret' "${temp_dir}/skopeo.log" >/dev/null
grep -F -- "oci:${layout_dir}:latest" "${temp_dir}/skopeo.log" >/dev/null
grep -F -- 'docker://ghcr.io/agentstation/neovex-machine-os:latest' "${temp_dir}/skopeo.log" >/dev/null
grep -F -- 'docker://ghcr.io/agentstation/neovex-machine-os:next' "${temp_dir}/skopeo.log" >/dev/null
test -f "${release_dir}/oci-layout-summary.txt"
test -f "${release_dir}/checksums.txt"
test -f "${release_dir}/publish-summary.txt"
grep -F 'image_reference=docker://ghcr.io/agentstation/neovex-machine-os:latest' "${release_dir}/publish-summary.txt" >/dev/null
grep -F 'additional_references=docker://ghcr.io/agentstation/neovex-machine-os:next' "${release_dir}/publish-summary.txt" >/dev/null

printf 'verified neovex machine-os publish wrapper\n'
