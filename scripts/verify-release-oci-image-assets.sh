#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-release-oci-image-assets.sh --artifacts-dir <dir> \
  [--expected-image <name>] [--expected-tag <vX.Y.Z>] [--checksums <path>] \
  [--require-license]

Verify the downloaded Nimbus OCI release evidence bundle:
- nimbus_oci_image.txt report is deterministic and internally consistent
- attestation, SBOM, and vulnerability scan assets are present and valid JSON
- attestation evidence mentions the expected image digest, repository,
  release workflow, SLSA predicate, tag ref, and verified timestamp evidence
- attestation evidence proves GitHub-hosted runner identity
- SBOM evidence names the expected image, tag, ref, digest, and both release
  platforms
- optional checksums-sha256.txt covers each nimbus_oci_* asset
- --require-license also requires a top-level LICENSE asset and checksum entry
EOF
}

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

require_file() {
  local path="$1"
  [[ -f "${path}" ]] || die "required file is missing: ${path}"
}

value_for() {
  local report="$1"
  local key="$2"
  awk -F= -v target="${key}" '$1 == target { print substr($0, length($1) + 2) }' "${report}" | tail -n 1
}

require_value() {
  local name="$1"
  local value="$2"
  [[ -n "${value}" ]] || die "${name} is required"
}

checksum_for() {
  local path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${path}" | awk '{print $1}'
  else
    shasum -a 256 "${path}" | awk '{print $1}'
  fi
}

assert_checksum_entry() {
  local checksums="$1"
  local path="$2"
  local name
  local expected
  local actual
  local expected_lower
  name="$(basename "${path}")"
  expected="$(awk -v name="${name}" '
    NF >= 2 {
      subject = $2
      sub(/^\*/, "", subject)
      if (subject == name) {
        count += 1
        digest = $1
      }
    }
    END {
      if (count == 0) {
        print "missing"
      } else if (count > 1) {
        print "duplicate"
      } else {
        print digest
      }
    }
  ' "${checksums}")"
  [[ "${expected}" != "missing" ]] || die "${checksums} does not contain ${name}"
  [[ "${expected}" != "duplicate" ]] || die "${checksums} contains duplicate entries for ${name}"
  [[ "${expected}" =~ ^[0-9A-Fa-f]{64}$ ]] || die "${checksums} contains malformed SHA-256 digest for ${name}: ${expected}"
  actual="$(checksum_for "${path}")"
  expected_lower="$(printf '%s' "${expected}" | tr 'A-F' 'a-f')"
  [[ "${actual}" == "${expected_lower}" ]] || die "${name} checksum mismatch: expected ${expected_lower}, got ${actual}"
}

jq_string_matches() {
  local path="$1"
  local pattern="$2"
  jq -e --arg pattern "${pattern}" '.. | strings | select(test($pattern))' "${path}" >/dev/null
}

jq_certificate_equals() {
  local path="$1"
  local expected="$2"
  jq -e --arg expected "${expected}" \
    '[.[]? | .verificationResult.signature.certificate? | .. | strings | select(. == $expected)] | length > 0' \
    "${path}" >/dev/null
}

jq_certificate_matches() {
  local path="$1"
  local pattern="$2"
  jq -e --arg pattern "${pattern}" \
    '[.[]? | .verificationResult.signature.certificate? | .. | strings | select(test($pattern))] | length > 0' \
    "${path}" >/dev/null
}

escape_regex() {
  printf '%s' "$1" | sed 's/[][\\.^$*+?{}()|]/\\&/g'
}

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
artifacts_dir=""
expected_image=""
expected_tag=""
checksums=""
require_license=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --artifacts-dir)
      artifacts_dir="${2:-}"
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
    --checksums)
      checksums="${2:-}"
      shift 2
      ;;
    --require-license)
      require_license=1
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

require_value "--artifacts-dir" "${artifacts_dir}"
[[ -d "${artifacts_dir}" ]] || die "artifacts dir does not exist: ${artifacts_dir}"
command -v jq >/dev/null 2>&1 || die "jq is required"
if [[ -z "${checksums}" && -f "${artifacts_dir}/checksums-sha256.txt" ]]; then
  checksums="${artifacts_dir}/checksums-sha256.txt"
fi

report="${artifacts_dir}/nimbus_oci_image.txt"
attestation="${artifacts_dir}/nimbus_oci_attestation.json"
sbom="${artifacts_dir}/nimbus_oci_sbom.json"
vulns="${artifacts_dir}/nimbus_oci_vulns.sarif.json"
license="${artifacts_dir}/LICENSE"

for path in "${report}" "${attestation}" "${sbom}" "${vulns}"; do
  require_file "${path}"
done

report_args=(--report "${report}")
if [[ -n "${expected_image}" ]]; then
  report_args+=(--expected-image "${expected_image}")
fi
if [[ -n "${expected_tag}" ]]; then
  report_args+=(--expected-tag "${expected_tag}")
fi
bash "${script_dir}/verify-release-oci-image-report.sh" "${report_args[@]}" >/dev/null

multi_digest="$(value_for "${report}" multi_arch_digest)"
multi_ref="$(value_for "${report}" multi_arch_ref)"
image="$(value_for "${report}" image)"
tag="$(value_for "${report}" tag)"
digest_hex="${multi_digest#sha256:}"
source_ref="refs/tags/${tag}"

require_value "multi_arch_digest" "${multi_digest}"
require_value "multi_arch_ref" "${multi_ref}"
require_value "image" "${image}"
require_value "tag" "${tag}"

case "${image}" in
  ghcr.io/*/*)
    image_without_registry="${image#ghcr.io/}"
    expected_repo="${image_without_registry%%/*}/${image_without_registry#*/}"
    ;;
  *)
    expected_repo="nimbus/nimbus"
    ;;
esac
expected_repo_url="github.com/${expected_repo}"
expected_workflow="${expected_repo_url}/.github/workflows/release.yml"
image_pattern="$(escape_regex "${image}")(:|@|$)"
repo_pattern='(^|/|https://)'"$(escape_regex "${expected_repo_url}")"'(/|$|@)|(^|[[:space:]])'"$(escape_regex "${expected_repo}")"'($|[[:space:]])'
workflow_pattern='(^|https://)'"$(escape_regex "${expected_workflow}")"'(@|$)|(^|[[:space:]])\.github/workflows/release\.yml(@|$)'

jq -e 'type == "array" and length > 0' "${attestation}" >/dev/null ||
  die "${attestation} must be a non-empty gh attestation verify JSON array"
jq -e --arg digest "${digest_hex}" '.. | strings | select(. == $digest or . == ("sha256:" + $digest))' "${attestation}" >/dev/null ||
  die "${attestation} does not mention ${multi_digest}"
jq -e '.. | strings | select(. == "https://slsa.dev/provenance/v1")' "${attestation}" >/dev/null ||
  die "${attestation} does not contain SLSA provenance verification evidence"
jq -e '[.[]? | .verificationResult.verifiedTimestamps? | arrays | select(length > 0)] | length > 0' "${attestation}" >/dev/null ||
  die "${attestation} does not contain verified timestamp evidence"
jq -e '[.[]? | .verificationResult.verifiedIdentity.runnerEnvironment? | select(. == "github-hosted")] | length > 0' "${attestation}" >/dev/null ||
  die "${attestation} does not contain verified GitHub-hosted runner identity"
jq -e '[.[]? | .verificationResult.signature.certificate.runnerEnvironment? | select(. == "github-hosted")] | length > 0' "${attestation}" >/dev/null ||
  die "${attestation} certificate does not mention GitHub-hosted runner identity"
jq_string_matches "${attestation}" "${image_pattern}" ||
  die "${attestation} does not mention image identity ${image}"
jq_certificate_equals "${attestation}" "${source_ref}" ||
  die "${attestation} certificate does not mention source ref ${source_ref}"
jq_certificate_matches "${attestation}" "${repo_pattern}" ||
  die "${attestation} certificate does not mention repository identity ${expected_repo}"
jq_certificate_matches "${attestation}" "${workflow_pattern}" ||
  die "${attestation} certificate does not mention release workflow identity ${expected_workflow}"

jq -e 'type == "object" and (.subject | type == "object")' "${sbom}" >/dev/null ||
  die "${sbom} must be an object with subject metadata"
jq -e --arg image "${image}" '.subject.image == $image' "${sbom}" >/dev/null ||
  die "${sbom} subject.image does not match ${image}"
jq -e --arg tag "${tag}" '.subject.tag == $tag' "${sbom}" >/dev/null ||
  die "${sbom} subject.tag does not match ${tag}"
jq -e --arg ref "${multi_ref}" '.subject.ref == $ref' "${sbom}" >/dev/null ||
  die "${sbom} subject.ref does not match ${multi_ref}"
jq -e --arg digest "${multi_digest}" '.subject.digest == $digest' "${sbom}" >/dev/null ||
  die "${sbom} subject.digest does not match ${multi_digest}"
for platform in "linux/amd64" "linux/arm64"; do
  jq -e --arg platform "${platform}" \
    '.platforms[$platform] != null and (.platforms[$platform] | type == "object") and (.platforms[$platform] | length > 0) and (.platforms[$platform].SPDXID | type == "string")' \
    "${sbom}" >/dev/null ||
    die "${sbom} must contain non-empty SPDX SBOM JSON for ${platform}"
done

jq -e '.version == "2.1.0" and (.runs | type == "array") and (.runs | length > 0)' "${vulns}" >/dev/null ||
  die "${vulns} must be a SARIF 2.1.0 report with at least one run"

if [[ "${require_license}" -eq 1 ]]; then
  require_file "${license}"
  [[ -s "${license}" ]] || die "${license} must be non-empty"
  repo_license="${script_dir}/../LICENSE"
  if [[ -f "${repo_license}" ]] && ! cmp -s "${repo_license}" "${license}"; then
    die "${license} does not match the repository LICENSE"
  fi
fi

if [[ -n "${checksums}" ]]; then
  require_file "${checksums}"
  for path in "${report}" "${attestation}" "${sbom}" "${vulns}"; do
    assert_checksum_entry "${checksums}" "${path}"
  done
  if [[ "${require_license}" -eq 1 ]]; then
    assert_checksum_entry "${checksums}" "${license}"
  fi
fi

if [[ "${require_license}" -eq 1 ]]; then
  printf 'verified: OCI image release assets in %s cover report, attestation, SBOM, vulnerability scan, LICENSE, and optional checksums for %s\n' "${artifacts_dir}" "${multi_ref}"
else
  printf 'verified: OCI image release assets in %s cover report, attestation, SBOM, vulnerability scan, and optional checksums for %s\n' "${artifacts_dir}" "${multi_ref}"
fi
