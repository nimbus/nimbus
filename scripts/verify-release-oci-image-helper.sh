#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-oci-image-helper.XXXXXX")"
trap 'rm -rf "${tmp_dir}"' EXIT

containerfile="${repo_root}/Containerfile"
workflow="${repo_root}/.github/workflows/release.yml"

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

assert_contains() {
  local path="$1"
  local needle="$2"
  grep -F -- "${needle}" "${path}" >/dev/null || die "${path} missing expected text: ${needle}"
}

extract_job() {
  local job="$1"
  local output="$2"
  awk -v marker="  ${job}:" '
    $0 == marker {
      printing = 1
      print
      next
    }
    printing && $0 ~ /^  [A-Za-z0-9_-]+:/ {
      exit
    }
    printing {
      print
    }
  ' "${workflow}" >"${output}"
  [[ -s "${output}" ]] || die "could not extract ${job} from ${workflow}"
}

assert_not_contains() {
  local path="$1"
  local needle="$2"
  if grep -Fi -- "${needle}" "${path}" >/dev/null; then
    die "${path} contains forbidden text: ${needle}"
  fi
}

assert_no_stale_nimbus_serve_command() {
  local path="$1"
  if grep -En -- 'nimbus(\.exe)?[[:space:]]+serve([^[:alnum:]_-]|$)' "${path}" >/dev/null; then
    die "${path} contains stale nimbus serve command guidance"
  fi
}

[[ -f "${containerfile}" ]] || die "Containerfile is required"
[[ -f "${workflow}" ]] || die "release workflow is required"

bash -n "${repo_root}/scripts/render-release-oci-image-report.sh"
bash -n "${repo_root}/scripts/verify-release-oci-image-report.sh"
bash -n "${repo_root}/scripts/verify-release-oci-image-assets.sh"
bash -n "${repo_root}/scripts/smoke-release-oci-image.sh"
bash -n "${repo_root}/scripts/verify-release-oci-image-live.sh"
bash -n "${repo_root}/scripts/verify-release-oci-image-live-helper.sh"

assert_contains "${containerfile}" "# syntax=docker/dockerfile:1@sha256:87999aa3d42bdc6bea60565083ee17e86d1f3339802f543c0d03998580f9cb89"
assert_contains "${containerfile}" "FROM debian:13-slim@sha256:b6e2a152f22a40ff69d92cb397223c906017e1391a73c952b588e51af8883bf8"
assert_contains "${containerfile}" "ARG NIMBUS_ARCHIVE"
assert_contains "${containerfile}" 'LABEL org.opencontainers.image.title="Nimbus"'
assert_contains "${containerfile}" 'org.opencontainers.image.source="${SOURCE_REPOSITORY}"'
assert_contains "${containerfile}" 'org.opencontainers.image.version="${NIMBUS_VERSION}"'
assert_contains "${containerfile}" 'org.opencontainers.image.revision="${VCS_REF}"'
assert_contains "${containerfile}" 'org.opencontainers.image.licenses="LicenseRef-Nimbus-Community"'
assert_contains "${containerfile}" 'COPY ${NIMBUS_ARCHIVE} /tmp/nimbus.tar.gz'
assert_contains "${containerfile}" "DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ca-certificates curl"
assert_contains "${containerfile}" "install -m 0755 /tmp/nimbus /usr/local/bin/nimbus"
assert_contains "${containerfile}" "install -m 0644 /tmp/LICENSE /usr/local/share/doc/nimbus/LICENSE"
assert_contains "${containerfile}" "USER 10001:10001"
assert_contains "${containerfile}" "EXPOSE 8080"
assert_contains "${containerfile}" 'VOLUME ["/var/lib/nimbus"]'
assert_contains "${containerfile}" 'HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 CMD ["curl", "-fsS", "http://127.0.0.1:8080/health"]'
assert_contains "${containerfile}" 'ENV HOME=/var/lib/nimbus'
assert_contains "${containerfile}" 'ENTRYPOINT ["nimbus"]'
assert_contains "${containerfile}" 'CMD ["start", "--host", "0.0.0.0", "--allow-network", "--data-dir", "/var/lib/nimbus/data", "--control-data-dir", "/var/lib/nimbus/control"]'

for forbidden in "cargo build" "target/release" "git clone" "systemd" "systemctl" "OpenRC" "supervisord" "systemd-run" "podman" "buildah" "conmon" "crun" "KVM" "qemu-system"; do
  assert_not_contains "${containerfile}" "${forbidden}"
done

assert_contains "${workflow}" "NIMBUS_IMAGE_PACKAGE: ghcr.io/nimbus/nimbus"
assert_contains "${workflow}" "TRIVY_IMAGE: ghcr.io/aquasecurity/trivy:0.69.3@sha256:bcc376de8d77cfe086a917230e818dc9f8528e3c852f7b1aff648949b6258d1c"
assert_contains "${workflow}" "build-oci-image:"
assert_contains "${workflow}" "publish-oci-image:"

build_job="${tmp_dir}/build-oci-image.yml"
publish_job="${tmp_dir}/publish-oci-image.yml"
release_job="${tmp_dir}/release.yml"
extract_job "build-oci-image" "${build_job}"
extract_job "publish-oci-image" "${publish_job}"
extract_job "release" "${release_job}"

for job_file in "${build_job}" "${publish_job}"; do
  assert_contains "${job_file}" "contents: read"
  assert_contains "${job_file}" "packages: write"
  assert_contains "${job_file}" "id-token: write"
  assert_contains "${job_file}" "attestations: write"
  assert_contains "${job_file}" "artifact-metadata: write"
  assert_not_contains "${job_file}" "contents: write"
  assert_not_contains "${job_file}" "actions: write"
  assert_not_contains "${job_file}" "security-events: write"
  assert_not_contains "${job_file}" "deployments: write"
  assert_not_contains "${job_file}" "cargo build"
  assert_not_contains "${job_file}" "target/release"
done

assert_contains "${build_job}" "actions/download-artifact@v8"
assert_contains "${build_job}" "tar -tzf"
assert_contains "${build_job}" 'NIMBUS_VERSION=${TAG}'
assert_contains "${build_job}" "--sbom=true"
assert_contains "${build_job}" "--provenance=mode=max"
assert_contains "${build_job}" "scripts/smoke-release-oci-image.sh"
assert_contains "${build_job}" "push-to-registry: true"

assert_contains "${publish_job}" "docker buildx imagetools create"
assert_contains "${publish_job}" "index:org.opencontainers.image.licenses=LicenseRef-Nimbus-Community"
assert_contains "${publish_job}" 'if [[ "${TAG}" != *-* ]]; then'
assert_contains "${publish_job}" "gh attestation verify"
assert_contains "${publish_job}" "--bundle-from-oci"
assert_contains "${publish_job}" "--signer-workflow"
assert_contains "${publish_job}" "--predicate-type https://slsa.dev/provenance/v1"
assert_contains "${publish_job}" "refs/tags/"
assert_contains "${publish_job}" "--deny-self-hosted-runners"
assert_contains "${publish_job}" "scripts/render-release-oci-image-report.sh"
assert_contains "${publish_job}" "scripts/verify-release-oci-image-report.sh"
assert_contains "${publish_job}" "scripts/verify-release-oci-image-assets.sh"
assert_contains "${publish_job}" "raw_sbom_amd64="
assert_contains "${publish_job}" "raw_sbom_arm64="
assert_contains "${publish_job}" '(index .SBOM "linux/amd64").SPDX'
assert_contains "${publish_job}" '(index .SBOM "linux/arm64").SPDX'
assert_contains "${publish_job}" "subject: {image: \$image, tag: \$tag, ref: \$ref, digest: \$digest}"
assert_contains "${publish_job}" '${TRIVY_IMAGE}'
assert_contains "${publish_job}" "--image-src remote"
assert_contains "${publish_job}" "--scanners vuln"
assert_contains "${publish_job}" "--skip-version-check"
assert_not_contains "${publish_job}" '${HOME}/.docker'
assert_contains "${release_job}" "scripts/verify-release-oci-image-assets.sh"
assert_contains "${release_job}" "--checksums artifacts/checksums-sha256.txt"
assert_contains "${release_job}" "--require-license"
assert_contains "${release_job}" 'if [[ "$TAG" == *-* ]]; then'
assert_contains "${release_job}" "cp LICENSE artifacts/LICENSE"
assert_contains "${release_job}" "test -s artifacts/LICENSE"
assert_contains "${release_job}" "checksum_assets=(nimbus_*)"
assert_contains "${release_job}" 'checksum_assets+=("${adapter_assets[@]}" install.sh LICENSE)'
assert_contains "${release_job}" "declare -A seen_checksum_asset=()"
assert_contains "${release_job}" "unique_checksum_assets=()"
assert_contains "${release_job}" 'seen_checksum_asset["$asset"]=1'
assert_contains "${release_job}" 'sha256sum "${unique_checksum_assets[@]}" > checksums-sha256.txt'
assert_contains "${release_job}" "Verify optional Bun/JSC adapter checksums"
assert_contains "${release_job}" "--checksums artifacts/checksums-sha256.txt"
assert_contains "${release_job}" "artifacts/LICENSE"
assert_contains "${release_job}" "Attest release asset provenance"
assert_contains "${release_job}" "subject-checksums: artifacts/checksums-sha256.txt"
assert_contains "${release_job}" "Attest checksum manifest provenance"
assert_contains "${release_job}" "subject-path: artifacts/checksums-sha256.txt"
assert_not_contains "${publish_job}" "*-alpha*"
assert_not_contains "${publish_job}" "*-beta*"
assert_not_contains "${publish_job}" "*-rc*"
assert_not_contains "${release_job}" "*-alpha*"
assert_not_contains "${release_job}" "*-beta*"
assert_not_contains "${release_job}" "*-rc*"
assert_contains "${workflow}" "nimbus_oci_image.txt"
assert_contains "${workflow}" "nimbus_oci_attestation.json"
assert_contains "${workflow}" "nimbus_oci_sbom.json"
assert_contains "${workflow}" "nimbus_oci_vulns.sarif.json"

live_script="${repo_root}/scripts/verify-release-oci-image-live.sh"
assert_contains "${live_script}" "gh release view"
assert_contains "${live_script}" "gh release download"
assert_contains "${live_script}" "checksums-sha256.txt"
assert_contains "${live_script}" "install.sh"
assert_contains "${live_script}" "LICENSE"
assert_contains "${live_script}" "nimbus_oci_image.txt"
assert_contains "${live_script}" "nimbus_oci_attestation.json"
assert_contains "${live_script}" "nimbus_oci_sbom.json"
assert_contains "${live_script}" "nimbus_oci_vulns.sarif.json"
assert_contains "${live_script}" "verify-release-archive-layout.sh"
assert_contains "${live_script}" "verify-release-oci-image-assets.sh"
assert_contains "${live_script}" "gh attestation verify"
assert_contains "${live_script}" "nimbus_release_asset_attestations.live.jsonl"
assert_contains "${live_script}" "--bundle-from-oci"
assert_contains "${live_script}" "--predicate-type https://slsa.dev/provenance/v1"
assert_contains "${live_script}" "--deny-self-hosted-runners"
assert_contains "${live_script}" "smoke-release-oci-image.sh"
assert_contains "${repo_root}/Makefile" "verify-release-oci-image-live:"
assert_contains "${repo_root}/Makefile" "set TAG=vX.Y.Z"
assert_contains "${repo_root}/Makefile" "verify-release-oci-image-live-helper:"
assert_contains "${repo_root}/scripts/verify-release-oci-image-live-helper.sh" "expected exactly one LICENSE release asset, found 0"
assert_contains "${repo_root}/scripts/verify-release-oci-image-live-helper.sh" "gh attestation verify"
assert_contains "${repo_root}/scripts/verify-release-oci-image-live-helper.sh" "nimbus-bun-jsc-adapter-*.tar.gz"
assert_contains "${repo_root}/scripts/verify-release-oci-image-live-helper.sh" "nimbus-extra+[proof].txt"
assert_contains "${repo_root}/scripts/verify-release-oci-image-live-helper.sh" "expected 13 release asset attestations"
assert_contains "${repo_root}/scripts/verify-release-oci-image-live-helper.sh" "complete release fixtures with optional adapter assets"

if grep -F -- "--privileged" "${workflow}" >/dev/null; then
  die "release workflow must not use privileged containers for the Nimbus application image"
fi

for current_guidance in \
  "${repo_root}/README.md" \
  "${repo_root}/docs/operating/container-image.md" \
  "${repo_root}/docs/operating/encryption.md" \
  "${repo_root}/docs/plans/windows-machine-support-plan.md" \
  "${repo_root}/docs/plans/research/bundle-distribution-from-object-storage.md" \
  "${repo_root}/docs/plans/research/runtime-file-storage-surface.md" \
  "${repo_root}/docs/plans/research/neovex-agent-prompt.md"; do
  assert_no_stale_nimbus_serve_command "${current_guidance}"
  assert_not_contains "${current_guidance}" "--privileged"
  assert_not_contains "${current_guidance}" "ghcr.io/nimbus/nimbus:latest"
done

good_report="${tmp_dir}/nimbus_oci_image.txt"
bad_report="${tmp_dir}/bad-nimbus_oci_image.txt"
multi_digest="sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
amd64_digest="sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
arm64_digest="sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"

bash "${repo_root}/scripts/render-release-oci-image-report.sh" \
  --image ghcr.io/nimbus/nimbus \
  --tag v9.9.9 \
  --multi-digest "${multi_digest}" \
  --amd64-digest "${amd64_digest}" \
  --arm64-digest "${arm64_digest}" \
  --latest-digest "${multi_digest}" \
  --output "${good_report}" \
  >"${tmp_dir}/render.out"

cat >"${tmp_dir}/expected.txt" <<EOF
# Nimbus OCI image release evidence
image=ghcr.io/nimbus/nimbus
tag=v9.9.9
multi_arch_digest=${multi_digest}
multi_arch_ref=ghcr.io/nimbus/nimbus:v9.9.9@${multi_digest}
linux_amd64_digest=${amd64_digest}
linux_amd64_ref=ghcr.io/nimbus/nimbus:v9.9.9-amd64@${amd64_digest}
linux_arm64_digest=${arm64_digest}
linux_arm64_ref=ghcr.io/nimbus/nimbus:v9.9.9-arm64@${arm64_digest}
latest_ref=ghcr.io/nimbus/nimbus:latest@${multi_digest}
entrypoint=["nimbus"]
default_command=["start","--host","0.0.0.0","--allow-network","--data-dir","/var/lib/nimbus/data","--control-data-dir","/var/lib/nimbus/control"]
state_volume=/var/lib/nimbus
http_port=8080
health_probe=/health
logging=stdout/stderr
signature=GitHub artifact attestation with Sigstore signature pushed to the registry
attestation_asset=nimbus_oci_attestation.json
signature_verify=gh attestation verify oci://ghcr.io/nimbus/nimbus:v9.9.9@${multi_digest} --repo nimbus/nimbus --bundle-from-oci --signer-workflow github.com/nimbus/nimbus/.github/workflows/release.yml --predicate-type https://slsa.dev/provenance/v1 --source-ref refs/tags/v9.9.9 --deny-self-hosted-runners
provenance=GitHub artifact attestation plus BuildKit SLSA provenance from docker buildx --provenance=mode=max
provenance_verify=gh attestation verify oci://ghcr.io/nimbus/nimbus:v9.9.9@${multi_digest} --repo nimbus/nimbus --bundle-from-oci --signer-workflow github.com/nimbus/nimbus/.github/workflows/release.yml --predicate-type https://slsa.dev/provenance/v1 --source-ref refs/tags/v9.9.9 --deny-self-hosted-runners --format json
sbom=BuildKit SBOM attestation from docker buildx --sbom=true
sbom_asset=nimbus_oci_sbom.json
sbom_platforms=linux/amd64,linux/arm64
sbom_verify=docker buildx imagetools inspect ghcr.io/nimbus/nimbus:v9.9.9@${multi_digest} --format '{{json .SBOM}}'
vulnerability_scan=Trivy SARIF report
vulnerability_scan_asset=nimbus_oci_vulns.sarif.json
vulnerability_scan_command=docker run --rm -v "\$PWD:/workspace" ghcr.io/aquasecurity/trivy:0.69.3@sha256:bcc376de8d77cfe086a917230e818dc9f8528e3c852f7b1aff648949b6258d1c image --image-src remote --scanners vuln --format sarif --output /workspace/nimbus_oci_vulns.sarif.json --no-progress --skip-version-check ghcr.io/nimbus/nimbus:v9.9.9@${multi_digest}
release_assets_verify=scripts/verify-release-oci-image-assets.sh --artifacts-dir <downloaded> --expected-image ghcr.io/nimbus/nimbus --expected-tag v9.9.9 --require-license --checksums <downloaded>/checksums-sha256.txt
smoke_command=bash scripts/smoke-release-oci-image.sh --image ghcr.io/nimbus/nimbus:v9.9.9@${multi_digest} --expected-version v9.9.9
pull_command=docker pull ghcr.io/nimbus/nimbus:v9.9.9@${multi_digest}
EOF

diff -u "${tmp_dir}/expected.txt" "${good_report}"

bash "${repo_root}/scripts/verify-release-oci-image-report.sh" \
  --report "${good_report}" \
  --expected-image ghcr.io/nimbus/nimbus \
  --expected-tag v9.9.9 \
  >"${tmp_dir}/verify-good.out"
grep -F "verified: OCI image report" "${tmp_dir}/verify-good.out" >/dev/null

cp "${good_report}" "${bad_report}"
sed -i.bak 's/^multi_arch_digest=sha256:aaaaaaaa/multi_arch_digest=sha256:00000000/' "${bad_report}"
rm -f "${bad_report}.bak"
if bash "${repo_root}/scripts/verify-release-oci-image-report.sh" \
  --report "${bad_report}" \
  --expected-image ghcr.io/nimbus/nimbus \
  --expected-tag v9.9.9 \
  >"${tmp_dir}/verify-bad.out" 2>&1; then
  die "expected OCI image report verifier to reject a mismatched multi-arch digest"
fi
grep -F "multi_arch_ref does not match" "${tmp_dir}/verify-bad.out" >/dev/null

cat >"${tmp_dir}/nimbus_oci_attestation.json" <<EOF
[
  {
    "verificationResult": {
      "signature": {
        "certificate": {
          "sourceRepository": "nimbus/nimbus",
          "sourceRepositoryURI": "https://github.com/nimbus/nimbus",
          "sourceRepositoryRef": "refs/tags/v9.9.9",
          "subjectAlternativeName": "https://github.com/nimbus/nimbus/.github/workflows/release.yml@refs/tags/v9.9.9",
          "runnerEnvironment": "github-hosted"
        }
      },
      "verifiedTimestamps": [
        {
          "timestamp": "2026-05-26T00:00:00Z"
        }
      ],
      "verifiedIdentity": {
        "runnerEnvironment": "github-hosted"
      },
      "statement": {
        "subject": [
          {
            "name": "ghcr.io/nimbus/nimbus:v9.9.9",
            "digest": {
              "sha256": "${multi_digest#sha256:}"
            }
          }
        ],
        "predicateType": "https://slsa.dev/provenance/v1",
        "predicate": {
          "builder": {
            "id": "https://github.com/nimbus/nimbus/.github/workflows/release.yml"
          },
          "metadata": {
            "sourceRef": "refs/tags/v9.9.9"
          }
        }
      }
    }
  }
]
EOF

cat >"${tmp_dir}/nimbus_oci_sbom.json" <<EOF
{
  "subject": {
    "image": "ghcr.io/nimbus/nimbus",
    "tag": "v9.9.9",
    "ref": "ghcr.io/nimbus/nimbus:v9.9.9@${multi_digest}",
    "digest": "${multi_digest}"
  },
  "platforms": {
    "linux/amd64": {
      "SPDXID": "SPDXRef-DOCUMENT"
    },
    "linux/arm64": {
      "SPDXID": "SPDXRef-DOCUMENT"
    }
  }
}
EOF

cat >"${tmp_dir}/nimbus_oci_vulns.sarif.json" <<'EOF'
{
  "version": "2.1.0",
  "runs": [
    {
      "tool": {
        "driver": {
          "name": "Trivy"
        }
      },
      "results": []
    }
  ]
}
EOF

bash "${repo_root}/scripts/verify-release-oci-image-assets.sh" \
  --artifacts-dir "${tmp_dir}" \
  --expected-image ghcr.io/nimbus/nimbus \
  --expected-tag v9.9.9 \
  >"${tmp_dir}/verify-assets.out"
grep -F "verified: OCI image release assets" "${tmp_dir}/verify-assets.out" >/dev/null

bad_source_ref_dir="${tmp_dir}/bad-source-ref"
mkdir "${bad_source_ref_dir}"
cp "${good_report}" "${bad_source_ref_dir}/nimbus_oci_image.txt"
cp "${tmp_dir}/nimbus_oci_sbom.json" "${bad_source_ref_dir}/nimbus_oci_sbom.json"
cp "${tmp_dir}/nimbus_oci_vulns.sarif.json" "${bad_source_ref_dir}/nimbus_oci_vulns.sarif.json"
sed 's|refs/tags/v9.9.9|refs/tags/v0.0.0|g' \
  "${tmp_dir}/nimbus_oci_attestation.json" >"${bad_source_ref_dir}/nimbus_oci_attestation.json"
if bash "${repo_root}/scripts/verify-release-oci-image-assets.sh" \
  --artifacts-dir "${bad_source_ref_dir}" \
  --expected-image ghcr.io/nimbus/nimbus \
  --expected-tag v9.9.9 \
  >"${tmp_dir}/verify-bad-source-ref.out" 2>&1; then
  die "expected OCI image asset verifier to reject attestation evidence for the wrong tag ref"
fi
grep -F "does not mention source ref refs/tags/v9.9.9" \
  "${tmp_dir}/verify-bad-source-ref.out" >/dev/null

bad_workflow_dir="${tmp_dir}/bad-workflow"
mkdir "${bad_workflow_dir}"
cp "${good_report}" "${bad_workflow_dir}/nimbus_oci_image.txt"
cp "${tmp_dir}/nimbus_oci_sbom.json" "${bad_workflow_dir}/nimbus_oci_sbom.json"
cp "${tmp_dir}/nimbus_oci_vulns.sarif.json" "${bad_workflow_dir}/nimbus_oci_vulns.sarif.json"
sed 's|github.com/nimbus/nimbus/.github/workflows/release.yml|github.com/nimbus/nimbus/.github/workflows/ci.yml|' \
  "${tmp_dir}/nimbus_oci_attestation.json" >"${bad_workflow_dir}/nimbus_oci_attestation.json"
if bash "${repo_root}/scripts/verify-release-oci-image-assets.sh" \
  --artifacts-dir "${bad_workflow_dir}" \
  --expected-image ghcr.io/nimbus/nimbus \
  --expected-tag v9.9.9 \
  >"${tmp_dir}/verify-bad-workflow.out" 2>&1; then
  die "expected OCI image asset verifier to reject attestation evidence for the wrong workflow"
fi
grep -F "does not mention release workflow identity" \
  "${tmp_dir}/verify-bad-workflow.out" >/dev/null

bad_timestamp_dir="${tmp_dir}/bad-timestamp"
mkdir "${bad_timestamp_dir}"
cp "${good_report}" "${bad_timestamp_dir}/nimbus_oci_image.txt"
cp "${tmp_dir}/nimbus_oci_sbom.json" "${bad_timestamp_dir}/nimbus_oci_sbom.json"
cp "${tmp_dir}/nimbus_oci_vulns.sarif.json" "${bad_timestamp_dir}/nimbus_oci_vulns.sarif.json"
jq 'del(.[]?.verificationResult.verifiedTimestamps)' \
  "${tmp_dir}/nimbus_oci_attestation.json" >"${bad_timestamp_dir}/nimbus_oci_attestation.json"
if bash "${repo_root}/scripts/verify-release-oci-image-assets.sh" \
  --artifacts-dir "${bad_timestamp_dir}" \
  --expected-image ghcr.io/nimbus/nimbus \
  --expected-tag v9.9.9 \
  >"${tmp_dir}/verify-bad-timestamp.out" 2>&1; then
  die "expected OCI image asset verifier to reject attestation evidence without verified timestamps"
fi
grep -F "verified timestamp" "${tmp_dir}/verify-bad-timestamp.out" >/dev/null

bad_runner_dir="${tmp_dir}/bad-runner"
mkdir "${bad_runner_dir}"
cp "${good_report}" "${bad_runner_dir}/nimbus_oci_image.txt"
cp "${tmp_dir}/nimbus_oci_sbom.json" "${bad_runner_dir}/nimbus_oci_sbom.json"
cp "${tmp_dir}/nimbus_oci_vulns.sarif.json" "${bad_runner_dir}/nimbus_oci_vulns.sarif.json"
jq '(.[]?.verificationResult.signature.certificate.runnerEnvironment) = "self-hosted" | (.[]?.verificationResult.verifiedIdentity.runnerEnvironment) = "self-hosted"' \
  "${tmp_dir}/nimbus_oci_attestation.json" >"${bad_runner_dir}/nimbus_oci_attestation.json"
if bash "${repo_root}/scripts/verify-release-oci-image-assets.sh" \
  --artifacts-dir "${bad_runner_dir}" \
  --expected-image ghcr.io/nimbus/nimbus \
  --expected-tag v9.9.9 \
  >"${tmp_dir}/verify-bad-runner.out" 2>&1; then
  die "expected OCI image asset verifier to reject non-GitHub-hosted runner evidence"
fi
grep -F "GitHub-hosted runner identity" "${tmp_dir}/verify-bad-runner.out" >/dev/null

bad_sbom_dir="${tmp_dir}/bad-sbom"
mkdir "${bad_sbom_dir}"
cp "${good_report}" "${bad_sbom_dir}/nimbus_oci_image.txt"
cp "${tmp_dir}/nimbus_oci_attestation.json" "${bad_sbom_dir}/nimbus_oci_attestation.json"
cp "${tmp_dir}/nimbus_oci_vulns.sarif.json" "${bad_sbom_dir}/nimbus_oci_vulns.sarif.json"
sed 's|"digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"|"digest": "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"|' \
  "${tmp_dir}/nimbus_oci_sbom.json" >"${bad_sbom_dir}/nimbus_oci_sbom.json"
if bash "${repo_root}/scripts/verify-release-oci-image-assets.sh" \
  --artifacts-dir "${bad_sbom_dir}" \
  --expected-image ghcr.io/nimbus/nimbus \
  --expected-tag v9.9.9 \
  >"${tmp_dir}/verify-bad-sbom.out" 2>&1; then
  die "expected OCI image asset verifier to reject SBOM evidence for the wrong digest"
fi
grep -F "subject.digest does not match ${multi_digest}" \
  "${tmp_dir}/verify-bad-sbom.out" >/dev/null

bad_platform_sbom_dir="${tmp_dir}/bad-platform-sbom"
mkdir "${bad_platform_sbom_dir}"
cp "${good_report}" "${bad_platform_sbom_dir}/nimbus_oci_image.txt"
cp "${tmp_dir}/nimbus_oci_attestation.json" "${bad_platform_sbom_dir}/nimbus_oci_attestation.json"
cp "${tmp_dir}/nimbus_oci_vulns.sarif.json" "${bad_platform_sbom_dir}/nimbus_oci_vulns.sarif.json"
jq 'del(.platforms["linux/arm64"])' \
  "${tmp_dir}/nimbus_oci_sbom.json" >"${bad_platform_sbom_dir}/nimbus_oci_sbom.json"
if bash "${repo_root}/scripts/verify-release-oci-image-assets.sh" \
  --artifacts-dir "${bad_platform_sbom_dir}" \
  --expected-image ghcr.io/nimbus/nimbus \
  --expected-tag v9.9.9 \
  >"${tmp_dir}/verify-bad-platform-sbom.out" 2>&1; then
  die "expected OCI image asset verifier to reject missing platform SBOM evidence"
fi
grep -F "linux/arm64" "${tmp_dir}/verify-bad-platform-sbom.out" >/dev/null

(
  cd "${tmp_dir}"
  cp "${repo_root}/LICENSE" LICENSE
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum nimbus_oci_image.txt nimbus_oci_attestation.json nimbus_oci_sbom.json nimbus_oci_vulns.sarif.json LICENSE > checksums-sha256.txt
  else
    shasum -a 256 nimbus_oci_image.txt nimbus_oci_attestation.json nimbus_oci_sbom.json nimbus_oci_vulns.sarif.json LICENSE > checksums-sha256.txt
  fi
)
bash "${repo_root}/scripts/verify-release-oci-image-assets.sh" \
  --artifacts-dir "${tmp_dir}" \
  --expected-image ghcr.io/nimbus/nimbus \
  --expected-tag v9.9.9 \
  --require-license \
  --checksums "${tmp_dir}/checksums-sha256.txt" \
  >"${tmp_dir}/verify-assets-checksums.out"
grep -F "verified: OCI image release assets" "${tmp_dir}/verify-assets-checksums.out" >/dev/null

duplicate_checksums_dir="${tmp_dir}/duplicate-checksums"
mkdir "${duplicate_checksums_dir}"
cp "${good_report}" "${duplicate_checksums_dir}/nimbus_oci_image.txt"
cp "${tmp_dir}/nimbus_oci_attestation.json" "${duplicate_checksums_dir}/nimbus_oci_attestation.json"
cp "${tmp_dir}/nimbus_oci_sbom.json" "${duplicate_checksums_dir}/nimbus_oci_sbom.json"
cp "${tmp_dir}/nimbus_oci_vulns.sarif.json" "${duplicate_checksums_dir}/nimbus_oci_vulns.sarif.json"
cp "${repo_root}/LICENSE" "${duplicate_checksums_dir}/LICENSE"
cp "${tmp_dir}/checksums-sha256.txt" "${duplicate_checksums_dir}/checksums-sha256.txt"
printf '%064d  nimbus_oci_image.txt\n' 0 >>"${duplicate_checksums_dir}/checksums-sha256.txt"
if bash "${repo_root}/scripts/verify-release-oci-image-assets.sh" \
  --artifacts-dir "${duplicate_checksums_dir}" \
  --expected-image ghcr.io/nimbus/nimbus \
  --expected-tag v9.9.9 \
  --require-license \
  --checksums "${duplicate_checksums_dir}/checksums-sha256.txt" \
  >"${tmp_dir}/verify-duplicate-checksums.out" 2>&1; then
  die "expected OCI image asset verifier to reject duplicate checksum entries"
fi
grep -F "duplicate entries" "${tmp_dir}/verify-duplicate-checksums.out" >/dev/null

malformed_checksums_dir="${tmp_dir}/malformed-checksums"
mkdir "${malformed_checksums_dir}"
cp "${good_report}" "${malformed_checksums_dir}/nimbus_oci_image.txt"
cp "${tmp_dir}/nimbus_oci_attestation.json" "${malformed_checksums_dir}/nimbus_oci_attestation.json"
cp "${tmp_dir}/nimbus_oci_sbom.json" "${malformed_checksums_dir}/nimbus_oci_sbom.json"
cp "${tmp_dir}/nimbus_oci_vulns.sarif.json" "${malformed_checksums_dir}/nimbus_oci_vulns.sarif.json"
cp "${repo_root}/LICENSE" "${malformed_checksums_dir}/LICENSE"
sed 's/^[0-9a-f][0-9a-f]*/not-a-sha256/' \
  "${tmp_dir}/checksums-sha256.txt" >"${malformed_checksums_dir}/checksums-sha256.txt"
if bash "${repo_root}/scripts/verify-release-oci-image-assets.sh" \
  --artifacts-dir "${malformed_checksums_dir}" \
  --expected-image ghcr.io/nimbus/nimbus \
  --expected-tag v9.9.9 \
  --require-license \
  --checksums "${malformed_checksums_dir}/checksums-sha256.txt" \
  >"${tmp_dir}/verify-malformed-checksums.out" 2>&1; then
  die "expected OCI image asset verifier to reject malformed checksum digests"
fi
grep -F "malformed SHA-256" "${tmp_dir}/verify-malformed-checksums.out" >/dev/null

bad_license_dir="${tmp_dir}/bad-license"
mkdir "${bad_license_dir}"
cp "${good_report}" "${bad_license_dir}/nimbus_oci_image.txt"
cp "${tmp_dir}/nimbus_oci_attestation.json" "${bad_license_dir}/nimbus_oci_attestation.json"
cp "${tmp_dir}/nimbus_oci_sbom.json" "${bad_license_dir}/nimbus_oci_sbom.json"
cp "${tmp_dir}/nimbus_oci_vulns.sarif.json" "${bad_license_dir}/nimbus_oci_vulns.sarif.json"
cp "${tmp_dir}/checksums-sha256.txt" "${bad_license_dir}/checksums-sha256.txt"
if bash "${repo_root}/scripts/verify-release-oci-image-assets.sh" \
  --artifacts-dir "${bad_license_dir}" \
  --expected-image ghcr.io/nimbus/nimbus \
  --expected-tag v9.9.9 \
  --require-license \
  --checksums "${bad_license_dir}/checksums-sha256.txt" \
  >"${tmp_dir}/verify-bad-license.out" 2>&1; then
  die "expected OCI image asset verifier to reject a final release bundle without LICENSE"
fi
grep -F "required file is missing" "${tmp_dir}/verify-bad-license.out" >/dev/null
grep -F "LICENSE" "${tmp_dir}/verify-bad-license.out" >/dev/null

assert_contains "${repo_root}/scripts/smoke-release-oci-image.sh" "auth rotate-admin"
assert_contains "${repo_root}/scripts/smoke-release-oci-image.sh" 'test "$(id -u)" = 10001'
assert_contains "${repo_root}/scripts/smoke-release-oci-image.sh" "/usr/local/share/doc/nimbus/LICENSE"
assert_contains "${repo_root}/scripts/smoke-release-oci-image.sh" "touch /var/lib/nimbus/.nimbus-write-test"
assert_contains "${repo_root}/scripts/smoke-release-oci-image.sh" "http://127.0.0.1:"
assert_contains "${repo_root}/scripts/smoke-release-oci-image.sh" "/health"
assert_contains "${repo_root}/scripts/smoke-release-oci-image.sh" "[[:space:]]*:[[:space:]]*true"
assert_contains "${repo_root}/scripts/smoke-release-oci-image.sh" "--entrypoint /bin/sh"

printf 'verified: release OCI image helper covers image definition, license evidence, least-permission workflow wiring, deterministic report rendering, release identity asset validation, stdout/stderr logging evidence, and smoke contract\n'
