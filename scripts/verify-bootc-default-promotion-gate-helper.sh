#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-bootc-default-promotion-gate.XXXXXX")"
trap 'rm -rf "${tmp_dir}"' EXIT
export NIMBUS_MACHINE_OS_SKIP_GHCR_PUBLIC_CHECK=1

release_dir="${tmp_dir}/release"
proof_dir="${tmp_dir}/proof"
bad_proof_dir="${tmp_dir}/bad-proof"
mkdir -p "${release_dir}" "${proof_dir}" "${bad_proof_dir}"

expected_tag="v9.9.9"
digest="sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
base_digest="sha256:1111111111111111111111111111111111111111111111111111111111111111"
bib_digest="sha256:2222222222222222222222222222222222222222222222222222222222222222"
manifest_digest="sha256:3333333333333333333333333333333333333333333333333333333333333333"
layer_digest="sha256:4444444444444444444444444444444444444444444444444444444444444444"
nimbus_binary_sha256="5555555555555555555555555555555555555555555555555555555555555555"

printf 'raw disk bytes\n' >"${release_dir}/nimbus-machine-os.raw.gz"
cat >"${release_dir}/nimbus-machine-os.sbom.cdx.json" <<EOF
{
  "bomFormat": "CycloneDX",
  "metadata": {"component": {"type": "application", "name": "nimbus-machine-os"}},
  "components": [
    {"type": "application", "name": "nimbus", "version": "${expected_tag}", "hashes": [{"alg": "SHA-256", "content": "${nimbus_binary_sha256}"}]},
    {"type": "application", "name": "podman"}
  ]
}
EOF
cat >"${release_dir}/build-summary.txt" <<EOF
candidate=direct-fedora-bootc
fedora_bootc_base_image=quay.io/fedora/fedora-bootc@${base_digest}
bib_image=quay.io/centos-bootc/bootc-image-builder@${bib_digest}
bootc_image_builder_rootfs=ext4
provisioning_contract=bootc-native-no-ignition-primary
selinux_expectation=container-runtime-domain-container-socket-policy-plus-fedora-bootupd-compat-plus-runtime-avc-gate
nimbus_version=${expected_tag}
nimbus_binary_sha256=${nimbus_binary_sha256}
EOF
cat >"${release_dir}/oci-layout-summary.txt" <<EOF
image_reference=docker://ghcr.io/nimbus/machine-os:${expected_tag}
disk_type=applehv
source_repository_url=https://github.com/nimbus/machine-os
nimbus_version=${expected_tag}
layer_digest=${layer_digest}
manifest_digest=${manifest_digest}
EOF
cat >"${release_dir}/publish-summary.txt" <<EOF
image_reference=docker://ghcr.io/nimbus/machine-os:${expected_tag}
image_digest=${digest}
image_digest_reference=ghcr.io/nimbus/machine-os:${expected_tag}@${digest}
EOF
cat >"${release_dir}/published-digests.txt" <<EOF
ghcr.io/nimbus/machine-os:${expected_tag}=${digest}
EOF
cat >"${release_dir}/machine-image-reference.txt" <<EOF
tag_reference=ghcr.io/nimbus/machine-os:${expected_tag}
digest_reference=ghcr.io/nimbus/machine-os:${expected_tag}@${digest}
digest=${digest}
EOF
(
  cd "${release_dir}"
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

cat >"${proof_dir}/summary.txt" <<EOF
output.dir                         ${proof_dir}
machine.name                       bootc-default-proof
privileged.guest_evidence          root-ssh port=10000 identity=${tmp_dir}/machine-key
selinux.avc_checker                ${repo_root}/../machine-os/scripts/check-selinux-avcs.sh
result                             captured
EOF
cat >"${proof_dir}/machine-status.txt" <<'EOF'
NAME                 LIFECYCLE MANAGER PROVIDER CPUS MEMORY(MiB) DISK(GiB) API
bootc-default-proof  running   ready   krunkit     4        4096        20 ready
EOF
cat >"${proof_dir}/machine-inspect.txt" <<EOF
{"config":{"guest":{"ssh_identity_path":"${tmp_dir}/machine-key"}},"state":{"runtime":{"ssh_port":10000}}}
EOF
cat >"${proof_dir}/guest-nimbus-version.txt" <<'EOF'
nimbus 9.9.9
EOF
cat >"${proof_dir}/guest-nimbus-sha256.txt" <<'EOF'
5555555555555555555555555555555555555555555555555555555555555555  /usr/local/bin/nimbus
EOF
cat >"${proof_dir}/guest-required-binaries.txt" <<'EOF'
present buildah /usr/bin/buildah
present conmon /usr/bin/conmon
present crun /usr/bin/crun
present netavark /usr/libexec/podman/netavark
present aardvark-dns /usr/libexec/podman/aardvark-dns
present fuse-overlayfs /usr/bin/fuse-overlayfs
EOF
cat >"${proof_dir}/guest-nimbus-socket-status.txt" <<'EOF'
SubState=listening
EOF
cat >"${proof_dir}/guest-nimbus-service-status.txt" <<'EOF'
SubState=running
EOF
cat >"${proof_dir}/guest-virtiofs-mount.txt" <<'EOF'
/Users nimbus-users virtiofs rw,nosuid,nodev
EOF
cat >"${proof_dir}/guest-machine-api-health.txt" <<'EOF'
HTTP/1.1 200 OK
{"status":"ok","role":"guest-machine-api","protocol_version":"v1alpha2"}
EOF
cat >"${proof_dir}/guest-machine-api-capabilities.txt" <<'EOF'
{"service_execution_ready":true,"service_execution_mode":"standard_containers","supported_operations":["service-sandboxes.image-start","service-sandboxes.stop","service-sandboxes.logs","os.bootc.status","os.bootc.switch","os.bootc.upgrade","os.bootc.rollback"]}
EOF
cat >"${proof_dir}/guest-machine-api-bootc-status.txt" <<'EOF'
HTTP/1.1 200 OK
{"status":{"status":{"booted":{"image":{"image":{"image":"ghcr.io/nimbus/machine-os:v9.9.9"},"imageDigest":"sha256:9999999999999999999999999999999999999999999999999999999999999999"}},"staged":null,"rollback":null}},"booted_image":"ghcr.io/nimbus/machine-os:v9.9.9","booted_digest":"sha256:9999999999999999999999999999999999999999999999999999999999999999","staged_image":null,"staged_digest":null,"rollback_image":null,"rollback_digest":null}
EOF
cat >"${proof_dir}/guest-bootc-status.txt" <<'EOF'
{"status":{"booted":{"image":{"image":{"image":"ghcr.io/nimbus/machine-os:v9.9.9"},"imageDigest":"sha256:9999999999999999999999999999999999999999999999999999999999999999"}},"staged":null,"rollback":null}}
EOF
cat >"${proof_dir}/guest-selinux-mode.txt" <<'EOF'
Enforcing
EOF
cat >"${proof_dir}/guest-package-context.txt" <<'EOF'
bootupd-0.2.33-1.fc44.aarch64
selinux-policy-targeted-44.1-1.fc44.noarch
systemd-259.5-1.fc44.aarch64
util-linux-core-2.41.4-7.fc44.aarch64
podman-5.8.2-1.fc44.aarch64
crun-1.27.1-1.fc44.aarch64
netavark-1.17.2-1.fc44.aarch64
aardvark-dns-1.17.2-1.fc44.aarch64
bootc-1.3.0-1.fc44.aarch64
policycoreutils-3.9-1.fc44.aarch64
ExecStart=/usr/bin/bootupctl update
EOF
cat >"${proof_dir}/guest-selinux-context.txt" <<'EOF'
system_u:system_r:container_runtime_t:s0 1000 ? 00:00:00 nimbus
system_u:object_r:container_var_run_t:s0 /run/nimbus/nimbus.sock
400 nimbus-machine-api cil
EOF
cat >"${proof_dir}/guest-selinux-avcs.txt" <<'EOF'
<no matches>
EOF
cat >"${proof_dir}/guest-selinux-avc-check.txt" <<'EOF'
verified SELinux AVC gate: no AVC denials in /tmp/proof/guest-selinux-avcs.txt
EOF
cat >"${proof_dir}/machine-log-tail.txt" <<'EOF'
nimbus bootc proof machine reached ready state
EOF

bash "${repo_root}/scripts/verify-bootc-default-promotion-gate.sh" \
  --release-dir "${release_dir}" \
  --guest-proof-dir "${proof_dir}" \
  --expected-tag "${expected_tag}" \
  >"${tmp_dir}/good.out"
grep -F "verified: bootc default promotion gate" "${tmp_dir}/good.out" >/dev/null

cp -R "${proof_dir}/." "${bad_proof_dir}/"
cat >"${bad_proof_dir}/guest-selinux-avc-check.txt" <<'EOF'
SELinux AVC gate failed for /tmp/proof/guest-selinux-avcs.txt
fedora_base_userdb_avcs=1
EOF
if bash "${repo_root}/scripts/verify-bootc-default-promotion-gate.sh" \
  --release-dir "${release_dir}" \
  --guest-proof-dir "${bad_proof_dir}" \
  --expected-tag "${expected_tag}" \
  >"${tmp_dir}/bad.out" 2>&1; then
  echo "expected promotion gate to reject a failed SELinux AVC proof" >&2
  exit 1
fi
grep -F "SELinux AVC check" "${tmp_dir}/bad.out" >/dev/null

printf 'verified: bootc default promotion gate helper accepts complete evidence and rejects failed SELinux proof\n'
