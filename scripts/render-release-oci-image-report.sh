#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: render-release-oci-image-report.sh --image <name> --tag <vX.Y.Z> \
  --multi-digest <sha256:...> --amd64-digest <sha256:...> \
  --arm64-digest <sha256:...> --output <path> [--latest-digest <sha256:...|none>]

Render the deterministic nimbus_oci_image.txt release asset.
EOF
}

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

require_value() {
  local name="$1"
  local value="$2"
  [[ -n "${value}" ]] || die "${name} is required"
}

assert_digest() {
  local name="$1"
  local value="$2"
  [[ "${value}" =~ ^sha256:[0-9a-f]{64}$ ]] || die "${name} must be sha256:<64 hex>, got '${value}'"
}

image=""
tag=""
multi_digest=""
amd64_digest=""
arm64_digest=""
latest_digest="none"
output=""
scanner_image="ghcr.io/aquasecurity/trivy:0.69.3@sha256:bcc376de8d77cfe086a917230e818dc9f8528e3c852f7b1aff648949b6258d1c"
sbom_asset="nimbus_oci_sbom.json"
vuln_asset="nimbus_oci_vulns.sarif.json"
attestation_asset="nimbus_oci_attestation.json"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --image)
      image="${2:-}"
      shift 2
      ;;
    --tag)
      tag="${2:-}"
      shift 2
      ;;
    --multi-digest)
      multi_digest="${2:-}"
      shift 2
      ;;
    --amd64-digest)
      amd64_digest="${2:-}"
      shift 2
      ;;
    --arm64-digest)
      arm64_digest="${2:-}"
      shift 2
      ;;
    --latest-digest)
      latest_digest="${2:-}"
      shift 2
      ;;
    --output)
      output="${2:-}"
      shift 2
      ;;
    --scanner-image)
      scanner_image="${2:-}"
      shift 2
      ;;
    --sbom-asset)
      sbom_asset="${2:-}"
      shift 2
      ;;
    --vulnerability-asset)
      vuln_asset="${2:-}"
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

require_value "--image" "${image}"
require_value "--tag" "${tag}"
require_value "--multi-digest" "${multi_digest}"
require_value "--amd64-digest" "${amd64_digest}"
require_value "--arm64-digest" "${arm64_digest}"
require_value "--output" "${output}"
require_value "--scanner-image" "${scanner_image}"
require_value "--sbom-asset" "${sbom_asset}"
require_value "--vulnerability-asset" "${vuln_asset}"

[[ "${tag}" == v* ]] || die "--tag must include the leading v"
[[ "${image}" != *:* ]] || die "--image must be an untagged package name, got ${image}"
assert_digest "--multi-digest" "${multi_digest}"
assert_digest "--amd64-digest" "${amd64_digest}"
assert_digest "--arm64-digest" "${arm64_digest}"
if [[ "${latest_digest}" != "none" ]]; then
  assert_digest "--latest-digest" "${latest_digest}"
fi

mkdir -p "$(dirname "${output}")"

multi_ref="${image}:${tag}@${multi_digest}"
amd64_ref="${image}:${tag}-amd64@${amd64_digest}"
arm64_ref="${image}:${tag}-arm64@${arm64_digest}"
if [[ "${latest_digest}" == "none" ]]; then
  latest_ref="none"
else
  latest_ref="${image}:latest@${latest_digest}"
fi

cat >"${output}" <<EOF
# Nimbus OCI image release evidence
image=${image}
tag=${tag}
multi_arch_digest=${multi_digest}
multi_arch_ref=${multi_ref}
linux_amd64_digest=${amd64_digest}
linux_amd64_ref=${amd64_ref}
linux_arm64_digest=${arm64_digest}
linux_arm64_ref=${arm64_ref}
latest_ref=${latest_ref}
entrypoint=["nimbus"]
default_command=["start","--host","0.0.0.0","--allow-network","--data-dir","/var/lib/nimbus/data","--control-data-dir","/var/lib/nimbus/control"]
state_volume=/var/lib/nimbus
http_port=8080
health_probe=/health
logging=stdout/stderr
signature=GitHub artifact attestation with Sigstore signature pushed to the registry
attestation_asset=${attestation_asset}
signature_verify=gh attestation verify oci://${multi_ref} --repo nimbus/nimbus --bundle-from-oci --signer-workflow github.com/nimbus/nimbus/.github/workflows/release.yml --predicate-type https://slsa.dev/provenance/v1 --source-ref refs/tags/${tag} --deny-self-hosted-runners
provenance=GitHub artifact attestation plus BuildKit SLSA provenance from docker buildx --provenance=mode=max
provenance_verify=gh attestation verify oci://${multi_ref} --repo nimbus/nimbus --bundle-from-oci --signer-workflow github.com/nimbus/nimbus/.github/workflows/release.yml --predicate-type https://slsa.dev/provenance/v1 --source-ref refs/tags/${tag} --deny-self-hosted-runners --format json
sbom=BuildKit SBOM attestation from docker buildx --sbom=true
sbom_asset=${sbom_asset}
sbom_platforms=linux/amd64,linux/arm64
sbom_verify=docker buildx imagetools inspect ${multi_ref} --format '{{json .SBOM}}'
vulnerability_scan=Trivy SARIF report
vulnerability_scan_asset=${vuln_asset}
vulnerability_scan_command=docker run --rm -v "\$PWD:/workspace" ${scanner_image} image --image-src remote --scanners vuln --format sarif --output /workspace/${vuln_asset} --no-progress --skip-version-check ${multi_ref}
release_assets_verify=scripts/verify-release-oci-image-assets.sh --artifacts-dir <downloaded> --expected-image ${image} --expected-tag ${tag} --require-license --checksums <downloaded>/checksums-sha256.txt
smoke_command=bash scripts/smoke-release-oci-image.sh --image ${multi_ref} --expected-version ${tag}
pull_command=docker pull ${multi_ref}
EOF

printf 'rendered: %s\n' "${output}"
