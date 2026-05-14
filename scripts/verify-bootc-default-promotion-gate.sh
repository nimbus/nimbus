#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-bootc-default-promotion-gate.sh --release-dir <path> --guest-proof-dir <path> --expected-tag <vX.Y.Z>

Verify that a Nimbus-owned bootc machine-os release bundle plus a real macOS
guest proof bundle are sufficient evidence to promote the bootc image as the
macOS default.

This gate intentionally composes two evidence classes:
- immutable release assets, digest references, SBOM, checksums, and AppleHV OCI metadata
- booted guest proof, machine API readiness, bootc/runtime capabilities, and clean SELinux AVC evidence

Options:
  --release-dir <path>      Directory containing downloaded machine-os release assets
  --guest-proof-dir <path>  Directory produced by collect-nimbus-machine-guest-proof.sh
  --expected-tag <tag>      Nimbus/machine-os release tag, for example v0.1.23
  -h, --help                Show this help
EOF
}

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

assert_file() {
  local path="$1"
  [[ -f "${path}" ]] || die "expected proof artifact missing: ${path}"
}

assert_contains() {
  local path="$1"
  local pattern="$2"
  local label="$3"
  grep -F "${pattern}" "${path}" >/dev/null || die "${label} missing '${pattern}' in ${path}"
}

assert_matches() {
  local path="$1"
  local pattern="$2"
  local label="$3"
  grep -E "${pattern}" "${path}" >/dev/null || die "${label} did not match /${pattern}/ in ${path}"
}

summary_value() {
  local summary_file="$1"
  local key="$2"
  awk -F= -v target="${key}" '$1 == target { print substr($0, length($1) + 2) }' "${summary_file}" | tail -n 1
}

assert_sha256_hex_value() {
  local label="$1"
  local value="$2"
  [[ "${value}" =~ ^[0-9a-f]{64}$ ]] || die "${label} must be 64 hex chars, got '${value}'"
}

release_dir=""
guest_proof_dir=""
expected_tag=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release-dir)
      release_dir="${2:-}"
      shift 2
      ;;
    --guest-proof-dir)
      guest_proof_dir="${2:-}"
      shift 2
      ;;
    --expected-tag)
      expected_tag="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[[ -n "${release_dir}" ]] || die "--release-dir is required"
[[ -n "${guest_proof_dir}" ]] || die "--guest-proof-dir is required"
[[ -n "${expected_tag}" ]] || die "--expected-tag is required"
[[ -d "${release_dir}" ]] || die "release dir does not exist: ${release_dir}"
[[ -d "${guest_proof_dir}" ]] || die "guest proof dir does not exist: ${guest_proof_dir}"

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
release_dir="$(cd "${release_dir}" && pwd)"
guest_proof_dir="$(cd "${guest_proof_dir}" && pwd)"

release_gate_args=(
  --release-dir "${release_dir}"
  --expected-tag "${expected_tag}"
)
if [[ "${NIMBUS_MACHINE_OS_SKIP_GHCR_PUBLIC_CHECK:-}" != "1" ]]; then
  release_gate_args+=(--require-ghcr-public)
fi

bash "${script_dir}/verify-machine-os-release-default-gate.sh" "${release_gate_args[@]}" >/dev/null

release_build_summary="${release_dir}/build-summary.txt"
release_nimbus_version="$(summary_value "${release_build_summary}" nimbus_version)"
release_nimbus_sha256="$(summary_value "${release_build_summary}" nimbus_binary_sha256)"
expected_guest_version="${expected_tag#v}"

[[ "${release_nimbus_version}" == "${expected_tag}" ]] || die "release build-summary nimbus_version expected ${expected_tag}, got ${release_nimbus_version}"
assert_sha256_hex_value "release build-summary nimbus_binary_sha256" "${release_nimbus_sha256}"

summary="${guest_proof_dir}/summary.txt"
machine_status="${guest_proof_dir}/machine-status.txt"
machine_inspect="${guest_proof_dir}/machine-inspect.txt"
guest_version="${guest_proof_dir}/guest-nimbus-version.txt"
guest_sha256="${guest_proof_dir}/guest-nimbus-sha256.txt"
guest_binaries="${guest_proof_dir}/guest-required-binaries.txt"
guest_socket_status="${guest_proof_dir}/guest-nimbus-socket-status.txt"
guest_service_status="${guest_proof_dir}/guest-nimbus-service-status.txt"
guest_virtiofs="${guest_proof_dir}/guest-virtiofs-mount.txt"
guest_health="${guest_proof_dir}/guest-machine-api-health.txt"
guest_capabilities="${guest_proof_dir}/guest-machine-api-capabilities.txt"
guest_machine_api_bootc_status="${guest_proof_dir}/guest-machine-api-bootc-status.txt"
guest_bootc_status="${guest_proof_dir}/guest-bootc-status.txt"
guest_selinux_mode="${guest_proof_dir}/guest-selinux-mode.txt"
guest_package_context="${guest_proof_dir}/guest-package-context.txt"
guest_selinux_context="${guest_proof_dir}/guest-selinux-context.txt"
guest_selinux_avcs="${guest_proof_dir}/guest-selinux-avcs.txt"
guest_selinux_avc_check="${guest_proof_dir}/guest-selinux-avc-check.txt"
machine_log_tail="${guest_proof_dir}/machine-log-tail.txt"

for artifact in \
  "${summary}" \
  "${machine_status}" \
  "${machine_inspect}" \
  "${guest_version}" \
  "${guest_sha256}" \
  "${guest_binaries}" \
  "${guest_socket_status}" \
  "${guest_service_status}" \
  "${guest_virtiofs}" \
  "${guest_health}" \
  "${guest_capabilities}" \
  "${guest_machine_api_bootc_status}" \
  "${guest_bootc_status}" \
  "${guest_selinux_mode}" \
  "${guest_package_context}" \
  "${guest_selinux_context}" \
  "${guest_selinux_avcs}" \
  "${guest_selinux_avc_check}" \
  "${machine_log_tail}"
do
  assert_file "${artifact}"
done

assert_contains "${summary}" "result" "guest proof summary"
assert_contains "${summary}" "captured" "guest proof summary"
assert_contains "${summary}" "privileged.guest_evidence" "guest proof privileged capture"
assert_contains "${summary}" "root-ssh" "guest proof privileged capture"

assert_matches "${machine_status}" "running|lifecycle:[[:space:]]*running" "machine status running"
assert_matches "${machine_status}" "ready|manager:[[:space:]]*ready" "machine status ready"
assert_contains "${machine_inspect}" "ssh_identity_path" "machine inspect SSH identity"
assert_contains "${machine_inspect}" "ssh_port" "machine inspect SSH port"

assert_contains "${guest_version}" "nimbus ${expected_guest_version}" "guest nimbus version"
assert_matches "${guest_sha256}" "[0-9a-f]{64}" "guest nimbus sha256"
guest_nimbus_sha256="$(awk '{ print $1; exit }' "${guest_sha256}")"
[[ "${guest_nimbus_sha256}" == "${release_nimbus_sha256}" ]] || die "guest nimbus SHA-256 ${guest_nimbus_sha256} does not match release build-summary nimbus_binary_sha256 ${release_nimbus_sha256}"
assert_contains "${guest_socket_status}" "SubState=listening" "nimbus.socket status"
assert_contains "${guest_service_status}" "SubState=running" "nimbus.service status"
assert_contains "${guest_virtiofs}" "virtiofs" "virtiofs mount"

for binary in buildah conmon crun netavark aardvark-dns fuse-overlayfs; do
  assert_contains "${guest_binaries}" "present ${binary}" "guest required binary ${binary}"
done

assert_matches "${guest_health}" '"status"[[:space:]]*:[[:space:]]*"ok"' "machine API health"
assert_contains "${guest_health}" "guest-machine-api" "machine API role"
assert_contains "${guest_health}" "v1alpha2" "machine API protocol"
assert_matches "${guest_capabilities}" '"service_execution_ready"[[:space:]]*:[[:space:]]*true' "machine API capabilities"
assert_matches "${guest_capabilities}" '"service_execution_mode"[[:space:]]*:[[:space:]]*"standard_containers"' "machine API service mode"
for operation in os.bootc.status os.bootc.switch os.bootc.upgrade os.bootc.rollback service-sandboxes.image-start service-sandboxes.stop service-sandboxes.logs; do
  assert_contains "${guest_capabilities}" "${operation}" "machine API operation ${operation}"
done
assert_contains "${guest_machine_api_bootc_status}" "HTTP/1.1 200 OK" "machine API bootc status"
assert_contains "${guest_machine_api_bootc_status}" "booted_image" "machine API bootc status"
assert_matches "${guest_machine_api_bootc_status}" '"booted_digest"[[:space:]]*:[[:space:]]*"sha256:[0-9a-f]{64}"' "machine API bootc status digest"
assert_contains "${guest_bootc_status}" '"booted"' "guest-local bootc status"
assert_contains "${guest_bootc_status}" '"image"' "guest-local bootc status"
assert_matches "${guest_bootc_status}" 'sha256:[0-9a-f]{64}' "guest-local bootc status digest"

assert_contains "${guest_selinux_mode}" "Enforcing" "SELinux enforcing mode"
for package in bootupd selinux-policy-targeted systemd util-linux-core podman crun netavark aardvark-dns bootc policycoreutils; do
  assert_matches "${guest_package_context}" "${package}-[0-9]" "package context ${package}"
done
assert_contains "${guest_package_context}" "ExecStart=/usr/bin/bootupctl update" "bootloader-update service"

assert_contains "${guest_selinux_context}" "container_runtime_t" "Nimbus service SELinux domain"
assert_contains "${guest_selinux_context}" "container_var_run_t" "Nimbus socket SELinux type"
assert_contains "${guest_selinux_context}" "nimbus-machine-api" "Nimbus SELinux module"

assert_contains "${guest_selinux_avc_check}" "verified SELinux AVC gate: no AVC denials" "SELinux AVC check"
assert_contains "${machine_log_tail}" "nimbus" "machine log tail"

printf 'verified: bootc default promotion gate has release evidence and clean macOS guest proof for %s\n' "${expected_tag}"
