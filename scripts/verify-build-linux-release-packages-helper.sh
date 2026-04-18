#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="$(mktemp -d "${TMPDIR:-/tmp}/neovex-linux-package-helper.XXXXXX")"
trap 'rm -rf "${output_dir}"' EXIT

neovex_stub="${output_dir}/neovex"
neovex_crun_stub="${output_dir}/neovex-crun"

cat >"${neovex_stub}" <<'EOF'
#!/bin/sh
printf 'neovex stub\n'
EOF

cat >"${neovex_crun_stub}" <<'EOF'
#!/bin/sh
printf 'neovex-crun stub\n'
EOF

chmod 0755 "${neovex_stub}" "${neovex_crun_stub}"

cd "${repo_root}"

bash scripts/build-linux-release-packages.sh \
  --output-dir "${output_dir}/render" \
  --neovex-binary "${neovex_stub}" \
  --neovex-crun-binary "${neovex_crun_stub}" \
  --version 0.1.10 \
  --crun-version 0.1.4 \
  --arch amd64 \
  --render-only \
  >"${output_dir}/render-summary.txt"

test -x "${output_dir}/render/staging/neovex/usr/bin/neovex"
test -x "${output_dir}/render/staging/neovex-crun/usr/libexec/neovex/crun"
test -f "${output_dir}/render/manifests/neovex-deb.yaml"
test -f "${output_dir}/render/manifests/neovex-rpm.yaml"
test -f "${output_dir}/render/manifests/neovex-crun-deb.yaml"
test -f "${output_dir}/render/manifests/neovex-crun-rpm.yaml"

grep -F "dst: /usr/bin/neovex" "${output_dir}/render/manifests/neovex-deb.yaml" >/dev/null
grep -F "dst: /usr/libexec/neovex/crun" "${output_dir}/render/manifests/neovex-crun-rpm.yaml" >/dev/null
grep -F "  - buildah" "${output_dir}/render/manifests/neovex-deb.yaml" >/dev/null
grep -F "  - conmon" "${output_dir}/render/manifests/neovex-deb.yaml" >/dev/null
grep -F "  - netavark" "${output_dir}/render/manifests/neovex-deb.yaml" >/dev/null
grep -F "  - aardvark-dns" "${output_dir}/render/manifests/neovex-deb.yaml" >/dev/null
grep -F "  - neovex-crun" "${output_dir}/render/manifests/neovex-deb.yaml" >/dev/null
grep -F "  - libkrun" "${output_dir}/render/manifests/neovex-crun-deb.yaml" >/dev/null
grep -F "  - libkrunfw" "${output_dir}/render/manifests/neovex-crun-deb.yaml" >/dev/null
grep -F "result=rendered" "${output_dir}/render-summary.txt" >/dev/null

if command -v nfpm >/dev/null 2>&1; then
  bash scripts/build-linux-release-packages.sh \
    --output-dir "${output_dir}/packaged" \
    --neovex-binary "${neovex_stub}" \
    --neovex-crun-binary "${neovex_crun_stub}" \
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
