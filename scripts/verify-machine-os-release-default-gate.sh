#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-machine-os-release-default-gate.sh --release-dir <path> --expected-tag <vX.Y.Z> [--require-ghcr-public]

Verify that a nimbus-machine-os release bundle is complete enough to be used
as the source of a future macOS default digest pin.

This intentionally verifies release evidence only. It does not prove macOS
boot parity or SELinux runtime safety; those remain separate BMD gates.

Required release assets:
- nimbus-machine-os.raw.gz
- nimbus-machine-os.sbom.cdx.json
- checksums.txt
- publish-summary.txt
- published-digests.txt
- machine-image-reference.txt

Options:
  --release-dir <path>     Directory containing downloaded machine-os assets
  --expected-tag <tag>     Nimbus/machine-os release tag, for example v0.1.23
  --require-ghcr-public    Require the GHCR image digest to be anonymously pullable
  -h, --help               Show this help
EOF
}

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

summary_value() {
  local summary_file="$1"
  local key="$2"
  awk -F= -v target="${key}" '$1 == target { print substr($0, length($1) + 2) }' "${summary_file}" | tail -n 1
}

assert_file() {
  local path="$1"
  [[ -f "${path}" ]] || die "expected release asset missing: ${path}"
}

assert_sha256_digest() {
  local label="$1"
  local value="$2"
  [[ "${value}" =~ ^sha256:[0-9a-f]{64}$ ]] || die "${label} must be sha256:<64 hex>, got '${value}'"
}

assert_sha256_hex() {
  local label="$1"
  local value="$2"
  [[ "${value}" =~ ^[0-9a-f]{64}$ ]] || die "${label} must be 64 hex chars, got '${value}'"
}

assert_ghcr_anonymous_pull() {
  local reference="$1"
  local digest="$2"
  local repository_path token_response token status manifest_url token_url

  command -v curl >/dev/null 2>&1 || die "curl is required for --require-ghcr-public"
  [[ "${reference}" == ghcr.io/* ]] || die "GHCR public check requires ghcr.io reference, got ${reference}"

  repository_path="${reference#ghcr.io/}"
  repository_path="${repository_path%%:*}"
  [[ -n "${repository_path}" ]] || die "failed to parse GHCR repository path from ${reference}"

  token_url="https://ghcr.io/token?service=ghcr.io&scope=repository:${repository_path}:pull"
  token_response="$(curl -sS "${token_url}")" || die "failed to request anonymous GHCR pull token for ${repository_path}"
  token="$(
    printf '%s\n' "${token_response}" |
      sed -nE 's/.*"(token|access_token)"[[:space:]]*:[[:space:]]*"([^"]+)".*/\2/p' |
      head -n 1
  )"

  [[ -n "${token}" ]] || die "GHCR package ${repository_path} is not anonymously readable; token endpoint did not issue a pull token"

  manifest_url="https://ghcr.io/v2/${repository_path}/manifests/${digest}"
  status="$(
    curl -sS -o /dev/null -w '%{http_code}' \
      -H "Authorization: Bearer ${token}" \
      -H "Accept: application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json" \
      "${manifest_url}"
  )" || die "failed to request GHCR manifest ${repository_path}@${digest}"

  [[ "${status}" == "200" ]] || die "GHCR manifest ${repository_path}@${digest} is not anonymously readable; got HTTP ${status}"
}

release_dir=""
expected_tag=""
require_ghcr_public=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release-dir)
      release_dir="${2:-}"
      shift 2
      ;;
    --expected-tag)
      expected_tag="${2:-}"
      shift 2
      ;;
    --require-ghcr-public)
      require_ghcr_public=1
      shift
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
[[ -n "${expected_tag}" ]] || die "--expected-tag is required"
[[ "${expected_tag}" == v* ]] || die "--expected-tag must include the leading v"
[[ -d "${release_dir}" ]] || die "release dir does not exist: ${release_dir}"

command -v sha256sum >/dev/null 2>&1 || die "sha256sum is required"

release_dir="$(cd "${release_dir}" && pwd)"
raw_disk="${release_dir}/nimbus-machine-os.raw.gz"
sbom="${release_dir}/nimbus-machine-os.sbom.cdx.json"
checksums="${release_dir}/checksums.txt"
publish_summary="${release_dir}/publish-summary.txt"
published_digests="${release_dir}/published-digests.txt"
machine_reference="${release_dir}/machine-image-reference.txt"
oci_summary="${release_dir}/oci-layout-summary.txt"
build_summary="${release_dir}/build-summary.txt"

assert_file "${raw_disk}"
assert_file "${sbom}"
assert_file "${checksums}"
assert_file "${publish_summary}"
assert_file "${published_digests}"
assert_file "${machine_reference}"
assert_file "${oci_summary}"
assert_file "${build_summary}"

(
  cd "${release_dir}"
  sha256sum -c checksums.txt >/dev/null
) || die "release checksums do not match downloaded machine-os assets"

tag_reference="$(summary_value "${machine_reference}" tag_reference)"
digest_reference="$(summary_value "${machine_reference}" digest_reference)"
digest="$(summary_value "${machine_reference}" digest)"
expected_reference="ghcr.io/nimbus/nimbus-machine-os:${expected_tag}"

[[ "${tag_reference}" == "${expected_reference}" ]] || die "machine-image-reference tag_reference expected ${expected_reference}, got ${tag_reference}"
assert_sha256_digest "machine-image-reference digest" "${digest}"
[[ "${digest_reference}" == "${expected_reference}@${digest}" ]] || die "machine-image-reference digest_reference must be ${expected_reference}@${digest}, got ${digest_reference}"

grep -F "${expected_reference}=${digest}" "${published_digests}" >/dev/null || \
  die "published-digests.txt does not contain ${expected_reference}=${digest}"

summary_image_reference="$(summary_value "${publish_summary}" image_reference)"
summary_digest="$(summary_value "${publish_summary}" image_digest)"
summary_digest_reference="$(summary_value "${publish_summary}" image_digest_reference)"
[[ "${summary_image_reference}" == "docker://${expected_reference}" ]] || die "publish-summary image_reference expected docker://${expected_reference}, got ${summary_image_reference}"
[[ "${summary_digest}" == "${digest}" ]] || die "publish-summary image_digest does not match machine-image-reference digest"
[[ "${summary_digest_reference}" == "${expected_reference}@${digest}" ]] || die "publish-summary image_digest_reference does not match machine-image-reference digest_reference"

[[ "$(summary_value "${oci_summary}" disk_type)" == "applehv" ]] || die "OCI layout summary must record disk_type=applehv"
[[ "$(summary_value "${oci_summary}" source_repository_url)" == "https://github.com/nimbus/nimbus-machine-os" ]] || die "OCI source repository must be nimbus/nimbus-machine-os"
[[ "$(summary_value "${oci_summary}" nimbus_version)" == "${expected_tag}" ]] || die "OCI nimbus_version must match ${expected_tag}"
assert_sha256_digest "OCI layer_digest" "$(summary_value "${oci_summary}" layer_digest)"
assert_sha256_digest "OCI manifest_digest" "$(summary_value "${oci_summary}" manifest_digest)"

[[ "$(summary_value "${build_summary}" candidate)" == "direct-fedora-bootc" ]] || die "build summary must record candidate=direct-fedora-bootc"
[[ "$(summary_value "${build_summary}" bootc_image_builder_rootfs)" == "ext4" ]] || die "build summary must record bootc_image_builder_rootfs=ext4"
[[ "$(summary_value "${build_summary}" provisioning_contract)" == "bootc-native-no-ignition-primary" ]] || die "build summary must record the bootc-native provisioning contract"
[[ "$(summary_value "${build_summary}" selinux_expectation)" == "container-runtime-domain-container-socket-policy-plus-fedora-bootupd-compat-plus-runtime-avc-gate" ]] || die "build summary must record the container-runtime SELinux policy, Fedora bootupd compatibility policy, and AVC promotion gate expectation"
[[ "$(summary_value "${build_summary}" nimbus_version)" == "${expected_tag}" ]] || die "build summary nimbus_version must match ${expected_tag}"
nimbus_binary_sha256="$(summary_value "${build_summary}" nimbus_binary_sha256)"
assert_sha256_hex "build summary nimbus_binary_sha256" "${nimbus_binary_sha256}"
assert_sha256_digest "base image digest" "$(summary_value "${build_summary}" fedora_bootc_base_image | sed 's/^.*@//')"
assert_sha256_digest "builder image digest" "$(summary_value "${build_summary}" bib_image | sed 's/^.*@//')"

grep -F '"bomFormat": "CycloneDX"' "${sbom}" >/dev/null || die "SBOM must be CycloneDX JSON"
grep -F '"name": "nimbus-machine-os"' "${sbom}" >/dev/null || die "SBOM must identify nimbus-machine-os"
grep -F '"name": "nimbus"' "${sbom}" >/dev/null || die "SBOM must include the embedded nimbus component"
grep -F "${expected_tag}" "${sbom}" >/dev/null || die "SBOM must include embedded nimbus version ${expected_tag}"
grep -F "${nimbus_binary_sha256}" "${sbom}" >/dev/null || die "SBOM must include embedded nimbus SHA-256 ${nimbus_binary_sha256}"
grep -F '"name": "podman"' "${sbom}" >/dev/null || die "SBOM must include podman"
grep -F "${digest}" "${machine_reference}" >/dev/null || die "machine-image-reference must include the promoted digest"

if [[ "${require_ghcr_public}" -eq 1 ]]; then
  assert_ghcr_anonymous_pull "${expected_reference}" "${digest}"
fi

printf 'verified: machine-os release %s has digest, embedded nimbus version/hash, SBOM, checksum, OCI, and bootc promotion evidence\n' "${expected_tag}"
