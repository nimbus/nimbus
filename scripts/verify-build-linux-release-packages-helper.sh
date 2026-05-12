#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-linux-package-helper.XXXXXX")"
trap 'rm -rf "${output_dir}"' EXIT

nimbus_stub="${output_dir}/nimbus"
nimbus_crun_stub="${output_dir}/nimbus-crun"

cat >"${nimbus_stub}" <<'EOF'
#!/bin/sh
printf 'nimbus stub\n'
EOF

cat >"${nimbus_crun_stub}" <<'EOF'
#!/bin/sh
printf 'nimbus-crun stub\n'
EOF

chmod 0755 "${nimbus_stub}" "${nimbus_crun_stub}"

cd "${repo_root}"

bash scripts/build-linux-release-packages.sh \
  --output-dir "${output_dir}/render" \
  --nimbus-binary "${nimbus_stub}" \
  --nimbus-crun-binary "${nimbus_crun_stub}" \
  --version 0.1.10 \
  --crun-version 0.1.4 \
  --arch amd64 \
  --render-only \
  >"${output_dir}/render-summary.txt"

test -x "${output_dir}/render/staging/nimbus/usr/bin/nimbus"
test -x "${output_dir}/render/staging/nimbus-crun/usr/libexec/nimbus/crun"
test -f "${output_dir}/render/manifests/nimbus-deb.yaml"
test -f "${output_dir}/render/manifests/nimbus-rpm.yaml"
test -f "${output_dir}/render/manifests/nimbus-crun-deb.yaml"
test -f "${output_dir}/render/manifests/nimbus-crun-rpm.yaml"

grep -F "dst: /usr/bin/nimbus" "${output_dir}/render/manifests/nimbus-deb.yaml" >/dev/null
grep -F "dst: /usr/libexec/nimbus/crun" "${output_dir}/render/manifests/nimbus-crun-rpm.yaml" >/dev/null
grep -F "  - buildah" "${output_dir}/render/manifests/nimbus-deb.yaml" >/dev/null
grep -F "  - conmon" "${output_dir}/render/manifests/nimbus-deb.yaml" >/dev/null
grep -F "  - netavark" "${output_dir}/render/manifests/nimbus-deb.yaml" >/dev/null
grep -F "  - aardvark-dns" "${output_dir}/render/manifests/nimbus-deb.yaml" >/dev/null
grep -F "  - nimbus-crun" "${output_dir}/render/manifests/nimbus-deb.yaml" >/dev/null
grep -F "  - libkrun" "${output_dir}/render/manifests/nimbus-crun-deb.yaml" >/dev/null
grep -F "  - libkrunfw" "${output_dir}/render/manifests/nimbus-crun-deb.yaml" >/dev/null
grep -F "result=rendered" "${output_dir}/render-summary.txt" >/dev/null

if command -v nfpm >/dev/null 2>&1; then
  bash scripts/build-linux-release-packages.sh \
    --output-dir "${output_dir}/packaged" \
    --nimbus-binary "${nimbus_stub}" \
    --nimbus-crun-binary "${nimbus_crun_stub}" \
    --version 0.1.10 \
    --crun-version 0.1.4 \
    --arch amd64 \
    >"${output_dir}/package-summary.txt"

  packaged_root="$(cd "${output_dir}/packaged" && pwd)"
  ls "${output_dir}/packaged"/packages/*.deb >/dev/null 2>&1
  ls "${output_dir}/packaged"/packages/*.rpm >/dev/null 2>&1
  test -f "${output_dir}/packaged/packages/checksums-sha256.txt"
  grep -F ".deb" "${output_dir}/packaged/packages/checksums-sha256.txt" >/dev/null
  grep -F ".rpm" "${output_dir}/packaged/packages/checksums-sha256.txt" >/dev/null
  grep -F "result=packaged" "${output_dir}/package-summary.txt" >/dev/null
  grep -F "packages.checksums=${packaged_root}/packages/checksums-sha256.txt" "${output_dir}/package-summary.txt" >/dev/null
  printf 'verified: linux package builder rendered and built deb/rpm artifacts\n'
else
  printf 'verified: linux package builder rendered deterministic deb/rpm manifests (nfpm not installed; package build skipped)\n'
fi
