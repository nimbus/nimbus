#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-release-oci-image-report.sh --report <path> [--expected-image <name>] [--expected-tag <vX.Y.Z>]

Verify the deterministic nimbus_oci_image.txt release asset.
EOF
}

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

value_for() {
  local key="$1"
  awk -F= -v target="${key}" '$1 == target { print substr($0, length($1) + 2) }' "${report}" | tail -n 1
}

require_key() {
  local key="$1"
  local value
  value="$(value_for "${key}")"
  [[ -n "${value}" ]] || die "missing ${key} in ${report}"
  printf '%s\n' "${value}"
}

assert_digest() {
  local key="$1"
  local value="$2"
  [[ "${value}" =~ ^sha256:[0-9a-f]{64}$ ]] || die "${key} must be sha256:<64 hex>, got '${value}'"
}

report=""
expected_image=""
expected_tag=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --report)
      report="${2:-}"
      shift 2
      ;;
    --expected-image)
      expected_image="${2:-}"
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

[[ -n "${report}" ]] || die "--report is required"
[[ -f "${report}" ]] || die "report does not exist: ${report}"

image="$(require_key image)"
tag="$(require_key tag)"
multi_digest="$(require_key multi_arch_digest)"
multi_ref="$(require_key multi_arch_ref)"
amd64_digest="$(require_key linux_amd64_digest)"
amd64_ref="$(require_key linux_amd64_ref)"
arm64_digest="$(require_key linux_arm64_digest)"
arm64_ref="$(require_key linux_arm64_ref)"
latest_ref="$(require_key latest_ref)"
entrypoint="$(require_key entrypoint)"
default_command="$(require_key default_command)"
state_volume="$(require_key state_volume)"
http_port="$(require_key http_port)"
health_probe="$(require_key health_probe)"
logging="$(require_key logging)"
signature_verify="$(require_key signature_verify)"
provenance_verify="$(require_key provenance_verify)"
sbom_verify="$(require_key sbom_verify)"
sbom_platforms="$(require_key sbom_platforms)"
vulnerability_scan_command="$(require_key vulnerability_scan_command)"
release_assets_verify="$(require_key release_assets_verify)"
smoke_command="$(require_key smoke_command)"
pull_command="$(require_key pull_command)"

[[ -z "${expected_image}" || "${image}" == "${expected_image}" ]] || die "image expected ${expected_image}, got ${image}"
[[ -z "${expected_tag}" || "${tag}" == "${expected_tag}" ]] || die "tag expected ${expected_tag}, got ${tag}"
[[ "${tag}" == v* ]] || die "tag must include leading v, got ${tag}"

assert_digest "multi_arch_digest" "${multi_digest}"
assert_digest "linux_amd64_digest" "${amd64_digest}"
assert_digest "linux_arm64_digest" "${arm64_digest}"

[[ "${multi_ref}" == "${image}:${tag}@${multi_digest}" ]] || die "multi_arch_ref does not match image/tag/digest"
[[ "${amd64_ref}" == "${image}:${tag}-amd64@${amd64_digest}" ]] || die "linux_amd64_ref does not match image/tag/digest"
[[ "${arm64_ref}" == "${image}:${tag}-arm64@${arm64_digest}" ]] || die "linux_arm64_ref does not match image/tag/digest"
if [[ "${latest_ref}" != "none" ]]; then
  [[ "${latest_ref}" == "${image}:latest@${multi_digest}" ]] || die "latest_ref must point at the multi-arch digest or be none"
fi

[[ "${entrypoint}" == '["nimbus"]' ]] || die "entrypoint must be [\"nimbus\"]"
[[ "${default_command}" == *'"start"'* ]] || die "default_command must start Nimbus"
[[ "${default_command}" == *'"0.0.0.0"'* ]] || die "default_command must bind the container interface"
[[ "${default_command}" == *'"--allow-network"'* ]] || die "default_command must opt into non-loopback bind"
[[ "${state_volume}" == "/var/lib/nimbus" ]] || die "state_volume must be /var/lib/nimbus"
[[ "${http_port}" == "8080" ]] || die "http_port must be 8080"
[[ "${health_probe}" == "/health" ]] || die "health_probe must be /health"
[[ "${logging}" == "stdout/stderr" ]] || die "logging must be stdout/stderr"
[[ "${sbom_platforms}" == "linux/amd64,linux/arm64" ]] || die "sbom_platforms must be linux/amd64,linux/arm64"

for command_value in \
  "${signature_verify}" \
  "${provenance_verify}" \
  "${sbom_verify}" \
  "${vulnerability_scan_command}" \
  "${smoke_command}" \
  "${pull_command}"; do
  [[ "${command_value}" == *"${multi_ref}"* ]] || die "verification command does not reference ${multi_ref}: ${command_value}"
done
for command_value in "${signature_verify}" "${provenance_verify}"; do
  [[ "${command_value}" == *"--repo nimbus/nimbus"* ]] || die "attestation command must pin the Nimbus repository: ${command_value}"
  [[ "${command_value}" == *"--bundle-from-oci"* ]] || die "attestation command must verify registry-pushed bundle evidence: ${command_value}"
  [[ "${command_value}" == *"--signer-workflow github.com/nimbus/nimbus/.github/workflows/release.yml"* ]] || die "attestation command must pin the release workflow identity: ${command_value}"
  [[ "${command_value}" == *"--predicate-type https://slsa.dev/provenance/v1"* ]] || die "attestation command must pin the SLSA provenance predicate: ${command_value}"
  [[ "${command_value}" == *"--source-ref refs/tags/${tag}"* ]] || die "attestation command must pin the release tag ref: ${command_value}"
  [[ "${command_value}" == *"--deny-self-hosted-runners"* ]] || die "attestation command must reject self-hosted-runner attestations: ${command_value}"
done

grep -F "sbom_asset=nimbus_oci_sbom.json" "${report}" >/dev/null || die "report must name nimbus_oci_sbom.json"
grep -F "attestation_asset=nimbus_oci_attestation.json" "${report}" >/dev/null || die "report must name nimbus_oci_attestation.json"
grep -F "vulnerability_scan_asset=nimbus_oci_vulns.sarif.json" "${report}" >/dev/null || die "report must name nimbus_oci_vulns.sarif.json"
[[ "${release_assets_verify}" == *"scripts/verify-release-oci-image-assets.sh"* ]] || die "release_assets_verify must use scripts/verify-release-oci-image-assets.sh"
[[ "${release_assets_verify}" == *"--expected-image ${image}"* ]] || die "release_assets_verify must pin expected image"
[[ "${release_assets_verify}" == *"--expected-tag ${tag}"* ]] || die "release_assets_verify must pin expected tag"
[[ "${release_assets_verify}" == *"--require-license"* ]] || die "release_assets_verify must require the release LICENSE asset"
[[ "${release_assets_verify}" == *"--checksums"* ]] || die "release_assets_verify must verify checksums"

printf 'verified: OCI image report %s records deterministic digest, attestation, SBOM, scan, smoke, and pull evidence for %s\n' "${report}" "${multi_ref}"
