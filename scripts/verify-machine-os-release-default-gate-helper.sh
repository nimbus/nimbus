#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-machine-os-release-gate.XXXXXX")"
trap 'rm -rf "${tmp_dir}"' EXIT

good_dir="${tmp_dir}/good"
bad_dir="${tmp_dir}/bad"
mkdir -p "${good_dir}" "${bad_dir}"

expected_tag="v9.9.9"
digest="sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
base_digest="sha256:1111111111111111111111111111111111111111111111111111111111111111"
bib_digest="sha256:2222222222222222222222222222222222222222222222222222222222222222"
manifest_digest="sha256:3333333333333333333333333333333333333333333333333333333333333333"
layer_digest="sha256:4444444444444444444444444444444444444444444444444444444444444444"
nimbus_binary_sha256="5555555555555555555555555555555555555555555555555555555555555555"

printf 'raw disk bytes\n' >"${good_dir}/nimbus-machine-os.raw.gz"
cat >"${good_dir}/nimbus-machine-os.sbom.cdx.json" <<EOF
{
  "bomFormat": "CycloneDX",
  "specVersion": "1.5",
  "version": 1,
  "metadata": {"component": {"type": "application", "name": "nimbus-machine-os"}},
  "components": [
    {"type": "application", "name": "nimbus", "version": "${expected_tag}", "hashes": [{"alg": "SHA-256", "content": "${nimbus_binary_sha256}"}]},
    {"type": "application", "name": "podman"}
  ]
}
EOF

cat >"${good_dir}/build-summary.txt" <<EOF
candidate=direct-fedora-bootc
fedora_bootc_base_image=quay.io/fedora/fedora-bootc@${base_digest}
bib_image=quay.io/centos-bootc/bootc-image-builder@${bib_digest}
bootc_image_builder_rootfs=ext4
provisioning_contract=bootc-native-no-ignition-primary
selinux_expectation=container-runtime-domain-container-socket-policy-plus-runtime-avc-gate
nimbus_version=${expected_tag}
nimbus_binary_sha256=${nimbus_binary_sha256}
EOF

cat >"${good_dir}/oci-layout-summary.txt" <<EOF
image_reference=docker://ghcr.io/nimbus/nimbus-machine-os:${expected_tag}
disk_type=applehv
source_repository_url=https://github.com/nimbus/nimbus-machine-os
nimbus_version=${expected_tag}
layer_digest=${layer_digest}
manifest_digest=${manifest_digest}
EOF

cat >"${good_dir}/publish-summary.txt" <<EOF
image_reference=docker://ghcr.io/nimbus/nimbus-machine-os:${expected_tag}
image_digest=${digest}
image_digest_reference=ghcr.io/nimbus/nimbus-machine-os:${expected_tag}@${digest}
EOF

cat >"${good_dir}/published-digests.txt" <<EOF
ghcr.io/nimbus/nimbus-machine-os:${expected_tag}=${digest}
EOF

cat >"${good_dir}/machine-image-reference.txt" <<EOF
tag_reference=ghcr.io/nimbus/nimbus-machine-os:${expected_tag}
digest_reference=ghcr.io/nimbus/nimbus-machine-os:${expected_tag}@${digest}
digest=${digest}
EOF

(
  cd "${good_dir}"
  sha256sum \
    nimbus-machine-os.raw.gz \
    nimbus-machine-os.sbom.cdx.json \
    build-summary.txt \
    oci-layout-summary.txt \
    publish-summary.txt \
    published-digests.txt \
    machine-image-reference.txt \
    >checksums.txt
)

bash "${repo_root}/scripts/verify-machine-os-release-default-gate.sh" \
  --release-dir "${good_dir}" \
  --expected-tag "${expected_tag}" \
  >"${tmp_dir}/good.out"
grep -F "verified: machine-os release ${expected_tag}" "${tmp_dir}/good.out" >/dev/null

cp -R "${good_dir}/." "${bad_dir}/"
sed -i.bak 's/disk_type=applehv/disk_type=raw/' "${bad_dir}/oci-layout-summary.txt"
rm -f "${bad_dir}/oci-layout-summary.txt.bak"
(
  cd "${bad_dir}"
  sha256sum \
    nimbus-machine-os.raw.gz \
    nimbus-machine-os.sbom.cdx.json \
    build-summary.txt \
    oci-layout-summary.txt \
    publish-summary.txt \
    published-digests.txt \
    machine-image-reference.txt \
    >checksums.txt
)

if bash "${repo_root}/scripts/verify-machine-os-release-default-gate.sh" \
  --release-dir "${bad_dir}" \
  --expected-tag "${expected_tag}" \
  >"${tmp_dir}/bad.out" 2>&1; then
  echo "expected machine-os release gate to reject disk_type=raw" >&2
  exit 1
fi
grep -F "disk_type=applehv" "${tmp_dir}/bad.out" >/dev/null

printf 'verified: machine-os release default gate helper accepts complete evidence and rejects non-applehv artifacts\n'
