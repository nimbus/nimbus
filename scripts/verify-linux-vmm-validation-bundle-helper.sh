#!/usr/bin/env bash
# shellcheck disable=SC2016 # Single-quoted probes must match literal generated variables.
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

mkdir -p "${fake_crun_source}/src/libcrun/handlers"
printf '/* source identity fixture */\n' > "${fake_crun_source}/src/libcrun/handlers/krun.c"

cat > "${fake_crun_asset}" <<'EOF'
#!/bin/sh
printf 'crun version 1.29.1\n+LIBKRUN\n'
EOF
chmod 0755 "${fake_crun_asset}"

fake_libkrun_root="${tmp_dir}/nimbus-libkrun"
mkdir -p "${fake_libkrun_root}/lib/pkgconfig" "${fake_libkrun_root}/include"
printf 'stub libkrun\n' > "${fake_libkrun_root}/lib/libkrun.so.1.19.4"
printf 'stub libkrunfw\n' > "${fake_libkrun_root}/lib/libkrunfw.so.5.5.0"
ln -s libkrun.so.1.19.4 "${fake_libkrun_root}/lib/libkrun.so.1"
ln -s libkrunfw.so.5.5.0 "${fake_libkrun_root}/lib/libkrunfw.so.5"
printf 'void krun_set_port_map_with_bind_address(void);\n' > "${fake_libkrun_root}/include/libkrun.h"
printf 'nimbus-libkrun=v1.19.4-nimbus.3\nlibkrunfw=5.5.0\n' > "${fake_libkrun_root}/NIMBUS_LIBKRUN_RELEASE.txt"
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

if bash "${repo_root}/scripts/prepare-linux-vmm-validation-bundle.sh" \
  --crun-source "${fake_crun_source}" \
  --output-root "${tmp_dir}/source-only-bundle" \
  > "${tmp_dir}/source-only-stdout.txt" \
  2> "${tmp_dir}/source-only-stderr.txt"; then
  echo "source-only validation bundle unexpectedly succeeded" >&2
  exit 70
fi
grep -F "provide released --nimbus-crun-asset plus --nimbus-libkrun-archive" \
  "${tmp_dir}/source-only-stderr.txt" >/dev/null

for expected_file in \
  "${bundle_root}/session.env" \
  "${bundle_root}/README.md" \
  "${bundle_root}/99-writeback-checklist.txt" \
  "${bundle_root}/commands/00-run-through-lh6.sh" \
  "${bundle_root}/commands/01-lh1-host-preflight.sh" \
  "${bundle_root}/commands/02-lh2-record-crun-source.sh" \
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

for generated_script in "${bundle_root}"/commands/*.sh; do
  bash -n "${generated_script}"
done

grep -F "CRUN_SOURCE=${fake_crun_source}" "${bundle_root}/session.env" >/dev/null
grep -F "NIMBUS_CRUN_ASSET=${fake_crun_asset}" "${bundle_root}/session.env" >/dev/null
grep -F "NIMBUS_LIBKRUN_ARCHIVE=${fake_libkrun_archive}" "${bundle_root}/session.env" >/dev/null
grep -F "INSTALL_PATH=/usr/libexec/nimbus/crun" "${bundle_root}/session.env" >/dev/null
grep -F "IMAGE_REF=docker.io/library/busybox:latest" "${bundle_root}/session.env" >/dev/null
grep -F "PROBE_URL=http://127.0.0.1:18080/" "${bundle_root}/session.env" >/dev/null

grep -F "bash ${bundle_root}/commands/01-lh1-host-preflight.sh" "${bundle_root}/commands/00-run-through-lh6.sh" >/dev/null
grep -F 'exec sudo -n -- bash "$0" "$@"' "${bundle_root}/commands/00-run-through-lh6.sh" >/dev/null
grep -F "scripts/check-vmm-host.sh" "${bundle_root}/commands/01-lh1-host-preflight.sh" >/dev/null
grep -F 'host_check_args+=(--allow-pending-private-runtime)' "${bundle_root}/commands/01-lh1-host-preflight.sh" >/dev/null
grep -F 'git -C "${CRUN_SOURCE}" rev-parse --verify HEAD' "${bundle_root}/commands/02-lh2-record-crun-source.sh" >/dev/null
grep -F 'git -C "${CRUN_SOURCE}" describe --always --dirty' "${bundle_root}/commands/02-lh2-record-crun-source.sh" >/dev/null
if bash "${bundle_root}/commands/02-lh2-record-crun-source.sh" \
  > "${tmp_dir}/source-identity-stdout.txt" \
  2> "${tmp_dir}/source-identity-stderr.txt"; then
  echo "source identity command unexpectedly accepted a source tree without Git identity" >&2
  exit 70
fi
if [[ -e "${bundle_root}/artifacts/lh2/crun-source.txt" ]]; then
  echo "source identity command wrote evidence after a Git lookup failed" >&2
  exit 70
fi
grep -F "stage.source=released-artifacts" "${bundle_root}/commands/03-lh3-build-stage-runtime.sh" >/dev/null
grep -F "stage.nimbus_libkrun_root=\${STAGE_DIR}" "${bundle_root}/commands/03-lh3-build-stage-runtime.sh" >/dev/null
grep -F "install.source=released-artifacts" "${bundle_root}/commands/04-lh3-install-private-runtime.sh" >/dev/null
grep -F 'exec sudo -n -- bash "$0" "$@"' "${bundle_root}/commands/04-lh3-install-private-runtime.sh" >/dev/null
grep -F 'tar -xzf "${NIMBUS_LIBKRUN_ARCHIVE}" -C /usr/libexec/nimbus' "${bundle_root}/commands/04-lh3-install-private-runtime.sh" >/dev/null
if grep -F 'sudo tar' "${bundle_root}/commands/04-lh3-install-private-runtime.sh" >/dev/null; then
  echo "generated install command must not require sudo after it becomes root" >&2
  exit 70
fi
grep -F "check-vmm-host-post-install.txt" "${bundle_root}/commands/05-lh4-verify-runtime-separation.sh" >/dev/null
grep -F 'scripts/check-vmm-host.sh' "${bundle_root}/commands/05-lh4-verify-runtime-separation.sh" >/dev/null
grep -F "scripts/prepare-krun-bundle.sh" "${bundle_root}/commands/07-lh5-prepare-krun-bundle.sh" >/dev/null
grep -F "buildah from --name" "${bundle_root}/commands/06-lh5-buildah-rootfs.sh" >/dev/null
grep -F 'exec sudo -n -- bash "$0" "$@"' "${bundle_root}/commands/06-lh5-buildah-rootfs.sh" >/dev/null
grep -F 'cp -a "${mounted_rootfs}/." "${copied_rootfs}/"' "${bundle_root}/commands/06-lh5-buildah-rootfs.sh" >/dev/null
if grep -F -- '--no-preserve=ownership' "${bundle_root}/commands/06-lh5-buildah-rootfs.sh" >/dev/null; then
  echo "generated rootfs copy must preserve OCI ownership" >&2
  exit 70
fi
grep -F "scripts/prepare-direct-krun-drill.sh" "${bundle_root}/commands/08-lh5-prepare-direct-drill.sh" >/dev/null
grep -F "scripts/prepare-conmon-krun-drill.sh" "${bundle_root}/commands/10-lh6-prepare-conmon-drill.sh" >/dev/null
grep -F 'bash "${START_CONTAINER}" 60' "${bundle_root}/commands/11-lh6-run-conmon-drill.sh" >/dev/null
grep -F 'start-container.txt' "${bundle_root}/commands/11-lh6-run-conmon-drill.sh" >/dev/null
grep -F "curl -fsS" "${bundle_root}/commands/11-lh6-run-conmon-drill.sh" >/dev/null
grep -F "${bundle_root}/artifacts/lh6/conmon-exit-status.txt" "${bundle_root}/99-writeback-checklist.txt" >/dev/null
grep -F "${bundle_root}/commands/12-cleanup-buildah-rootfs.sh" "${bundle_root}/README.md" >/dev/null
grep -F 'prefix each command' "${bundle_root}/README.md" >/dev/null
grep -F 'exec sudo -n -- bash "$0" "$@"' "${bundle_root}/commands/12-cleanup-buildah-rootfs.sh" >/dev/null
grep -F '"${INSTALL_PATH}" delete -f "${DIRECT_CONTAINER_ID}"' "${bundle_root}/commands/12-cleanup-buildah-rootfs.sh" >/dev/null

echo "verified: linux vmm validation bundle helper generated deterministic LH1-LH6 command scripts, released-runtime install path, source diagnostics, and checklist"
