#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-linux-package-helper.XXXXXX")"
trap 'rm -rf "${output_dir}"' EXIT

nimbus_stub="${output_dir}/nimbus"
nimbus_libkrun_archive="${output_dir}/nimbus-libkrun-linux-amd64.tar.gz"
nimbus_crun_stub="${output_dir}/nimbus-crun"

make_libkrun_archive() {
  local archive_path="$1"
  local staging_dir

  staging_dir="$(mktemp -d "${output_dir}/nimbus-libkrun.XXXXXX")"
  mkdir -p "${staging_dir}/lib/pkgconfig" "${staging_dir}/include"
  printf 'stub libkrun\n' >"${staging_dir}/lib/libkrun.so.1.17.4"
  printf 'stub libkrunfw\n' >"${staging_dir}/lib/libkrunfw.so.5.3.0"
  ln -s libkrun.so.1.17.4 "${staging_dir}/lib/libkrun.so.1"
  ln -s libkrun.so.1 "${staging_dir}/lib/libkrun.so"
  ln -s libkrunfw.so.5.3.0 "${staging_dir}/lib/libkrunfw.so.5"
  ln -s libkrunfw.so.5 "${staging_dir}/lib/libkrunfw.so"
  printf 'void krun_set_port_map_with_bind_address(void);\n' >"${staging_dir}/include/libkrun.h"
  printf 'prefix=/usr/libexec/nimbus\nlibdir=${prefix}/lib\n' >"${staging_dir}/lib/pkgconfig/libkrun.pc"
  printf 'nimbus-libkrun=v1.17.4-nimbus.1\n' >"${staging_dir}/NIMBUS_LIBKRUN_RELEASE.txt"
  COPYFILE_DISABLE=1 COPY_EXTENDED_ATTRIBUTES_DISABLE=1 tar -czf "${archive_path}" -C "${staging_dir}" .
}

cat >"${nimbus_stub}" <<'EOF'
#!/bin/sh
printf 'nimbus stub\n'
EOF

cat >"${nimbus_crun_stub}" <<'EOF'
#!/bin/sh
printf 'nimbus-crun stub\n'
EOF

chmod 0755 "${nimbus_stub}" "${nimbus_crun_stub}"
make_libkrun_archive "${nimbus_libkrun_archive}"

cd "${repo_root}"

bash scripts/build-linux-release-packages.sh \
  --output-dir "${output_dir}/render" \
  --nimbus-binary "${nimbus_stub}" \
  --nimbus-libkrun-archive "${nimbus_libkrun_archive}" \
  --nimbus-crun-binary "${nimbus_crun_stub}" \
  --version 0.1.10 \
  --libkrun-version 1.17.4-nimbus.1 \
  --crun-version 0.1.4 \
  --arch amd64 \
  --render-only \
  >"${output_dir}/render-summary.txt"

test -x "${output_dir}/render/staging/nimbus/usr/bin/nimbus"
test -e "${output_dir}/render/staging/nimbus-libkrun/usr/libexec/nimbus/lib/libkrun.so.1"
test -e "${output_dir}/render/staging/nimbus-libkrun/usr/libexec/nimbus/lib/libkrunfw.so.5"
test -f "${output_dir}/render/staging/nimbus-libkrun/usr/libexec/nimbus/NIMBUS_LIBKRUN_RELEASE.txt"
test -x "${output_dir}/render/staging/nimbus-crun/usr/libexec/nimbus/crun"
test -f "${output_dir}/render/manifests/nimbus-deb.yaml"
test -f "${output_dir}/render/manifests/nimbus-rpm.yaml"
test -f "${output_dir}/render/manifests/nimbus-libkrun-deb.yaml"
test -f "${output_dir}/render/manifests/nimbus-libkrun-rpm.yaml"
test -f "${output_dir}/render/manifests/nimbus-crun-deb.yaml"
test -f "${output_dir}/render/manifests/nimbus-crun-rpm.yaml"

grep -F "dst: /usr/bin/nimbus" "${output_dir}/render/manifests/nimbus-deb.yaml" >/dev/null
grep -F "dst: /usr/libexec/nimbus/crun" "${output_dir}/render/manifests/nimbus-crun-rpm.yaml" >/dev/null
grep -F "  - buildah" "${output_dir}/render/manifests/nimbus-deb.yaml" >/dev/null
grep -F "  - conmon" "${output_dir}/render/manifests/nimbus-deb.yaml" >/dev/null
grep -F "  - netavark" "${output_dir}/render/manifests/nimbus-deb.yaml" >/dev/null
grep -F "  - aardvark-dns" "${output_dir}/render/manifests/nimbus-deb.yaml" >/dev/null
grep -F "  - nimbus-crun" "${output_dir}/render/manifests/nimbus-deb.yaml" >/dev/null
grep -F "dst: /usr/libexec/nimbus/lib" "${output_dir}/render/manifests/nimbus-libkrun-deb.yaml" >/dev/null
grep -F "dst: /usr/libexec/nimbus/NIMBUS_LIBKRUN_RELEASE.txt" "${output_dir}/render/manifests/nimbus-libkrun-deb.yaml" >/dev/null
grep -F "  - nimbus-libkrun" "${output_dir}/render/manifests/nimbus-crun-deb.yaml" >/dev/null
if grep -F "  - libkrun" "${output_dir}/render/manifests/nimbus-crun-deb.yaml" >/dev/null; then
  echo "nimbus-crun manifest still depends on distro libkrun" >&2
  exit 1
fi
if grep -F "  - libkrunfw" "${output_dir}/render/manifests/nimbus-crun-deb.yaml" >/dev/null; then
  echo "nimbus-crun manifest still depends on distro libkrunfw" >&2
  exit 1
fi
grep -F "result=rendered" "${output_dir}/render-summary.txt" >/dev/null

if command -v nfpm >/dev/null 2>&1; then
  bash scripts/build-linux-release-packages.sh \
    --output-dir "${output_dir}/packaged" \
    --nimbus-binary "${nimbus_stub}" \
    --nimbus-libkrun-archive "${nimbus_libkrun_archive}" \
    --nimbus-crun-binary "${nimbus_crun_stub}" \
    --version 0.1.10 \
    --libkrun-version 1.17.4-nimbus.1 \
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
  printf 'verified: linux package builder rendered deterministic nimbus/nimbus-libkrun/nimbus-crun deb/rpm manifests (nfpm not installed; package build skipped)\n'
fi
