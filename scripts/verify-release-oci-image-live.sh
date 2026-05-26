#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-release-oci-image-live.sh --tag <vX.Y.Z> [options]

Download and verify a published Nimbus release plus its GHCR OCI image.

Options:
  --repo <owner/name>       GitHub release repository (default: nimbus/nimbus)
  --image <ref>            OCI image package (default: ghcr.io/nimbus/nimbus)
  --output-dir <dir>       Empty directory for downloaded release evidence
  --runtime docker|podman  Container runtime for smoke test (default: docker)
  --skip-smoke             Skip the container runtime smoke test

The verifier checks:
- expected GitHub Release assets, including LICENSE and nimbus_oci_* evidence
- archive layout and whole-release checksum coverage
- release-asset attestations for every checksummed asset plus the manifest
- registry-pushed GitHub/Sigstore attestation for the image digest
- published image smoke test unless --skip-smoke is set
EOF
}

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "$1 is required"
}

require_file() {
  local path="$1"
  [[ -f "${path}" ]] || die "required release asset missing after download: ${path}"
  [[ -s "${path}" ]] || die "release asset is empty: ${path}"
}

release_asset_count() {
  local release_json="$1"
  local asset="$2"
  jq -r --arg name "${asset}" '[.assets[]? | select(.name == $name)] | length' <<<"${release_json}"
}

checksum_file_has_subject() {
  local checksums="$1"
  local subject="$2"
  awk -v expected="${subject}" '
    NF >= 2 {
      asset = $2
      sub(/^\*/, "", asset)
      if (asset == expected) {
        found = 1
      }
    }
    END {
      exit found ? 0 : 1
    }
  ' "${checksums}"
}

report_value() {
  local report="$1"
  local key="$2"
  awk -F= -v target="${key}" '$1 == target { print substr($0, length($1) + 2) }' "${report}" | tail -n 1
}

repo="nimbus/nimbus"
image="ghcr.io/nimbus/nimbus"
tag=""
output_dir=""
runtime="docker"
skip_smoke=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)
      repo="${2:-}"
      shift 2
      ;;
    --image)
      image="${2:-}"
      shift 2
      ;;
    --tag)
      tag="${2:-}"
      shift 2
      ;;
    --output-dir)
      output_dir="${2:-}"
      shift 2
      ;;
    --runtime)
      runtime="${2:-}"
      shift 2
      ;;
    --skip-smoke)
      skip_smoke=1
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

[[ -n "${tag}" ]] || die "--tag is required"
[[ "${tag}" == v* ]] || die "--tag must include the leading v"
[[ -n "${repo}" ]] || die "--repo is required"
[[ -n "${image}" ]] || die "--image is required"
case "${runtime}" in
  docker|podman) ;;
  *) die "--runtime must be docker or podman" ;;
esac

require_command gh
require_command jq
require_command awk

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ -z "${output_dir}" ]]; then
  output_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-release-oci-live.XXXXXX")"
else
  mkdir -p "${output_dir}"
  if [[ -n "$(find "${output_dir}" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
    die "--output-dir must be empty: ${output_dir}"
  fi
fi
output_dir="$(cd "${output_dir}" && pwd)"

release_json="$(gh release view "${tag}" --repo "${repo}" --json tagName,url,assets,isDraft,isPrerelease)"
actual_tag="$(jq -r '.tagName' <<<"${release_json}")"
[[ "${actual_tag}" == "${tag}" ]] || die "release tag mismatch: expected ${tag}, got ${actual_tag}"
jq -e '.isDraft == false' <<<"${release_json}" >/dev/null || die "${tag} is still a draft release"

expected_assets=(
  checksums-sha256.txt
  install.sh
  LICENSE
  nimbus_darwin_arm64.tar.gz
  nimbus_linux_arm64.tar.gz
  nimbus_linux_x86_64.tar.gz
  nimbus_windows_x86_64.zip
  nimbus_oci_image.txt
  nimbus_oci_attestation.json
  nimbus_oci_sbom.json
  nimbus_oci_vulns.sarif.json
)

for asset in "${expected_assets[@]}"; do
  count="$(release_asset_count "${release_json}" "${asset}")"
  [[ "${count}" == "1" ]] || die "expected exactly one ${asset} release asset, found ${count}"
done

gh release download "${tag}" --repo "${repo}" --dir "${output_dir}"

for asset in "${expected_assets[@]}"; do
  require_file "${output_dir}/${asset}"
done

checksums="${output_dir}/checksums-sha256.txt"
checksum_assets=()
while read -r digest asset extra; do
  [[ -n "${digest}" ]] || continue
  [[ -z "${extra:-}" ]] || die "checksum entry must have exactly two fields: ${digest} ${asset} ${extra}"
  [[ "${digest}" =~ ^[0-9A-Fa-f]{64}$ ]] || die "checksum entry has malformed SHA-256 digest for ${asset}: ${digest}"
  [[ "${asset}" != */* ]] || die "checksum entry must be a release asset basename: ${asset}"
  require_file "${output_dir}/${asset}"
  count="$(release_asset_count "${release_json}" "${asset}")"
  [[ "${count}" == "1" ]] || die "checksum entry ${asset} is not a unique GitHub Release asset"
  checksum_assets+=("${asset}")
done <"${checksums}"

duplicate_checksum_asset="$(
  awk 'NF { print $2 }' "${checksums}" | sort | uniq -d | head -n 1
)"
[[ -z "${duplicate_checksum_asset}" ]] ||
  die "checksums-sha256.txt contains duplicate entries for ${duplicate_checksum_asset}"

uncovered_release_asset="$(
  jq -r '.assets[]?.name' <<<"${release_json}" |
    while IFS= read -r release_asset; do
      [[ "${release_asset}" == "checksums-sha256.txt" ]] && continue
      checksum_file_has_subject "${checksums}" "${release_asset}" ||
        printf '%s\n' "${release_asset}"
    done |
    head -n 1
)"
[[ -z "${uncovered_release_asset}" ]] ||
  die "release asset is not covered by checksums-sha256.txt: ${uncovered_release_asset}"

if command -v sha256sum >/dev/null 2>&1; then
  (cd "${output_dir}" && sha256sum -c checksums-sha256.txt >/dev/null)
else
  (cd "${output_dir}" && shasum -a 256 -c checksums-sha256.txt >/dev/null)
fi

bash "${script_dir}/verify-release-archive-layout.sh" \
  --artifacts-dir "${output_dir}"

bash "${script_dir}/verify-bun-jsc-release-assets.sh" \
  --artifacts-dir "${output_dir}" \
  --checksums "${checksums}"

bash "${script_dir}/verify-release-oci-image-assets.sh" \
  --artifacts-dir "${output_dir}" \
  --expected-image "${image}" \
  --expected-tag "${tag}" \
  --require-license \
  --checksums "${checksums}"

signer_workflow="github.com/${repo}/.github/workflows/release.yml"
release_asset_attestations="${output_dir}/nimbus_release_asset_attestations.live.jsonl"
: >"${release_asset_attestations}"
attested_assets=(checksums-sha256.txt)
for asset in "${checksum_assets[@]}"; do
  attested_assets+=("${asset}")
done
for asset in "${attested_assets[@]}"; do
  asset_attestation="$(mktemp "${TMPDIR:-/tmp}/nimbus-release-asset-attestation.XXXXXX")"
  gh attestation verify \
    "${output_dir}/${asset}" \
    --repo "${repo}" \
    --signer-workflow "${signer_workflow}" \
    --predicate-type https://slsa.dev/provenance/v1 \
    --source-ref "refs/tags/${tag}" \
    --deny-self-hosted-runners \
    --format json \
    > "${asset_attestation}"
  jq -c --arg asset "${asset}" '{asset: $asset, attestations: .}' \
    "${asset_attestation}" >>"${release_asset_attestations}"
  rm -f "${asset_attestation}"
done

report="${output_dir}/nimbus_oci_image.txt"
multi_ref="$(report_value "${report}" multi_arch_ref)"
multi_digest="$(report_value "${report}" multi_arch_digest)"
[[ "${multi_ref}" == "${image}:${tag}@${multi_digest}" ]] ||
  die "report multi_arch_ref does not match expected image/tag/digest"
[[ "${multi_digest}" =~ ^sha256:[0-9a-f]{64}$ ]] ||
  die "report multi_arch_digest is not a valid digest: ${multi_digest}"

live_attestation="${output_dir}/nimbus_oci_attestation.live.json"
gh attestation verify \
  "oci://${multi_ref}" \
  --repo "${repo}" \
  --bundle-from-oci \
  --signer-workflow "${signer_workflow}" \
  --predicate-type https://slsa.dev/provenance/v1 \
  --source-ref "refs/tags/${tag}" \
  --deny-self-hosted-runners \
  --format json \
  > "${live_attestation}"

live_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-release-oci-live-attestation.XXXXXX")"
trap 'rm -rf "${live_dir}"' EXIT
cp "${output_dir}/nimbus_oci_image.txt" "${live_dir}/nimbus_oci_image.txt"
cp "${live_attestation}" "${live_dir}/nimbus_oci_attestation.json"
cp "${output_dir}/nimbus_oci_sbom.json" "${live_dir}/nimbus_oci_sbom.json"
cp "${output_dir}/nimbus_oci_vulns.sarif.json" "${live_dir}/nimbus_oci_vulns.sarif.json"
cp "${output_dir}/LICENSE" "${live_dir}/LICENSE"

bash "${script_dir}/verify-release-oci-image-assets.sh" \
  --artifacts-dir "${live_dir}" \
  --expected-image "${image}" \
  --expected-tag "${tag}" \
  --require-license

if [[ "${skip_smoke}" -eq 0 ]]; then
  bash "${script_dir}/smoke-release-oci-image.sh" \
    --image "${multi_ref}" \
    --expected-version "${tag}" \
    --runtime "${runtime}"
fi

release_url="$(jq -r '.url' <<<"${release_json}")"
printf 'verified: %s release %s and OCI image %s with evidence in %s\n' \
  "${repo}" "${release_url}" "${multi_ref}" "${output_dir}"
