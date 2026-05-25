#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_base="${TMPDIR:-/tmp}"
tmp_base="${tmp_base%/}"
tmp_dir="$(mktemp -d "${tmp_base}/nimbus-linux-vmm-bundle-verify.XXXXXX")"
tmp_dir="$(cd "${tmp_dir}" && pwd)"
trap 'rm -rf "${tmp_dir}"' EXIT

fake_crun_source="${tmp_dir}/crun-source"
fake_crun_asset="${tmp_dir}/nimbus-crun-linux-amd64"
fake_libkrun_archive="${tmp_dir}/nimbus-libkrun-linux-amd64.tar.gz"
bundle_root="${tmp_dir}/bundle"

mkdir -p "${fake_crun_source}"

cat > "${fake_crun_asset}" <<'EOF'
#!/bin/sh
printf 'crun version 1.27.1\n+LIBKRUN\n'
EOF
chmod 0755 "${fake_crun_asset}"

fake_libkrun_root="${tmp_dir}/nimbus-libkrun"
mkdir -p "${fake_libkrun_root}/lib/pkgconfig" "${fake_libkrun_root}/include"
printf 'stub libkrun\n' > "${fake_libkrun_root}/lib/libkrun.so.1.18.1"
printf 'stub libkrunfw\n' > "${fake_libkrun_root}/lib/libkrunfw.so.5.3.0"
ln -s libkrun.so.1.18.1 "${fake_libkrun_root}/lib/libkrun.so.1"
ln -s libkrunfw.so.5.3.0 "${fake_libkrun_root}/lib/libkrunfw.so.5"
printf 'void krun_set_port_map_with_bind_address(void);\n' > "${fake_libkrun_root}/include/libkrun.h"
printf 'nimbus-libkrun=v1.18.1-nimbus.1\n' > "${fake_libkrun_root}/NIMBUS_LIBKRUN_RELEASE.txt"
COPYFILE_DISABLE=1 COPY_EXTENDED_ATTRIBUTES_DISABLE=1 tar -czf "${fake_libkrun_archive}" -C "${fake_libkrun_root}" .

bash "${repo_root}/scripts/prepare-linux-vmm-validation-bundle.sh" \
  --crun-source "${fake_crun_source}" \
  --nimbus-crun-asset "${fake_crun_asset}" \
  --nimbus-libkrun-archive "${fake_libkrun_archive}" \
  --output-root "${bundle_root}" \
  --image docker.io/library/busybox:latest \
  --host-port 18080 \
  --guest-port 8080 \
  > "${tmp_dir}/stdout.txt"

for expected_file in \
  "${bundle_root}/session.env" \
  "${bundle_root}/README.md" \
  "${bundle_root}/99-writeback-checklist.txt" \
  "${bundle_root}/commands/00-run-through-lh6.sh" \
  "${bundle_root}/commands/01-lh1-host-preflight.sh" \
  "${bundle_root}/commands/02-lh2-verify-crun-patch.sh" \
  "${bundle_root}/commands/03-lh3-build-stage-runtime.sh" \
  "${bundle_root}/commands/04-lh3-install-private-runtime.sh" \
  "${bundle_root}/commands/05-lh4-verify-runtime-separation.sh" \
  "${bundle_root}/commands/06-lh5-buildah-rootfs.sh" \
  "${bundle_root}/commands/07-lh5-prepare-krun-bundle.sh" \
  "${bundle_root}/commands/08-lh5-prepare-direct-drill.sh" \
  "${bundle_root}/commands/09-lh5-run-direct-drill.sh" \
  "${bundle_root}/commands/10-lh6-prepare-conmon-drill.sh" \
  "${bundle_root}/commands/11-lh6-run-conmon-drill.sh" \
  "${bundle_root}/commands/12-cleanup-buildah-rootfs.sh"
do
  if [[ ! -f "${expected_file}" ]]; then
    echo "expected bundle file missing: ${expected_file}" >&2
    exit 70
  fi
done

grep -F "CRUN_SOURCE=${fake_crun_source}" "${bundle_root}/session.env" >/dev/null
grep -F "NIMBUS_CRUN_ASSET=${fake_crun_asset}" "${bundle_root}/session.env" >/dev/null
grep -F "NIMBUS_LIBKRUN_ARCHIVE=${fake_libkrun_archive}" "${bundle_root}/session.env" >/dev/null
grep -F "INSTALL_PATH=/usr/libexec/nimbus/crun" "${bundle_root}/session.env" >/dev/null
grep -F "IMAGE_REF=docker.io/library/busybox:latest" "${bundle_root}/session.env" >/dev/null
grep -F "PROBE_URL=http://127.0.0.1:18080/" "${bundle_root}/session.env" >/dev/null

grep -F "bash ${bundle_root}/commands/01-lh1-host-preflight.sh" "${bundle_root}/commands/00-run-through-lh6.sh" >/dev/null
grep -F "scripts/check-vmm-host.sh" "${bundle_root}/commands/01-lh1-host-preflight.sh" >/dev/null
grep -F "scripts/verify-crun-patch.sh" "${bundle_root}/commands/02-lh2-verify-crun-patch.sh" >/dev/null
grep -F "using released runtime artifacts" "${bundle_root}/commands/02-lh2-verify-crun-patch.sh" >/dev/null
grep -F "scripts/build-nimbus-crun.sh" "${bundle_root}/commands/03-lh3-build-stage-runtime.sh" >/dev/null
grep -F "stage.source=released-artifacts" "${bundle_root}/commands/03-lh3-build-stage-runtime.sh" >/dev/null
grep -F "stage.nimbus_libkrun_root=\${STAGE_DIR}" "${bundle_root}/commands/03-lh3-build-stage-runtime.sh" >/dev/null
grep -F "install.source=released-artifacts" "${bundle_root}/commands/04-lh3-install-private-runtime.sh" >/dev/null
grep -F "sudo tar -xzf" "${bundle_root}/commands/04-lh3-install-private-runtime.sh" >/dev/null
grep -F "scripts/prepare-krun-bundle.sh" "${bundle_root}/commands/07-lh5-prepare-krun-bundle.sh" >/dev/null
grep -F "buildah from --name" "${bundle_root}/commands/06-lh5-buildah-rootfs.sh" >/dev/null
grep -F "scripts/prepare-direct-krun-drill.sh" "${bundle_root}/commands/08-lh5-prepare-direct-drill.sh" >/dev/null
grep -F "scripts/prepare-conmon-krun-drill.sh" "${bundle_root}/commands/10-lh6-prepare-conmon-drill.sh" >/dev/null
grep -F "curl -fsS" "${bundle_root}/commands/11-lh6-run-conmon-drill.sh" >/dev/null
grep -F "${bundle_root}/artifacts/lh6/conmon-exit-status.txt" "${bundle_root}/99-writeback-checklist.txt" >/dev/null
grep -F "${bundle_root}/commands/12-cleanup-buildah-rootfs.sh" "${bundle_root}/README.md" >/dev/null

echo "verified: linux vmm validation bundle helper generated deterministic LH1-LH6 command scripts, released-runtime install path, source diagnostics, and checklist"
