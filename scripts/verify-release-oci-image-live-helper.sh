#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "${repo_root}/scripts/bun-jsc-adapter-contract.sh"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-oci-live-helper.XXXXXX")"
trap 'rm -rf "${tmp_dir}"' EXIT

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "$1 is required"
}

write_stub_gh() {
  local path="$1"
  cat >"${path}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

printf 'gh %s\n' "$*" >>"${NIMBUS_LIVE_GH_LOG:?}"

case "$1 $2" in
  "release view")
    tag="$3"
    assets="$(
      find "${NIMBUS_LIVE_FIXTURE_ARTIFACTS:?}" -maxdepth 1 -type f -print |
        sort |
        while IFS= read -r asset; do
          jq -n --arg name "$(basename "${asset}")" '{name: $name}'
        done |
        jq -s .
    )"
    jq -n \
      --arg tag "${tag}" \
      --arg url "https://github.com/nimbus/nimbus/releases/tag/${tag}" \
      --argjson assets "${assets}" \
      '{tagName: $tag, url: $url, isDraft: false, isPrerelease: false, assets: $assets}'
    ;;
  "release download")
    shift 2
    tag="$1"
    shift
    dir=""
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --dir)
          dir="${2:-}"
          shift 2
          ;;
        *)
          shift
          ;;
      esac
    done
    [[ -n "${dir}" ]] || {
      echo "stub gh release download missing --dir for ${tag}" >&2
      exit 1
    }
    cp "${NIMBUS_LIVE_FIXTURE_ARTIFACTS:?}"/* "${dir}/"
    ;;
  "attestation verify")
    args=" $* "
    if [[ "${args}" == *" oci://ghcr.io/nimbus/nimbus:v9.9.9@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa "* ]]; then
      image_attestation=1
    else
      image_attestation=0
      expected_asset=0
      while IFS= read -r asset_path; do
        case "${args}" in
          *"${asset_path}"*) expected_asset=1 ;;
        esac
      done < <(find "${NIMBUS_LIVE_DOWNLOAD_DIR:?}" -maxdepth 1 -type f -print | sort)
      if [[ "${expected_asset}" -ne 1 ]]; then
        echo "stub gh attestation verify did not receive an expected release asset path: ${args}" >&2
        exit 1
      fi
    fi

    required_flags=(
      "--repo nimbus/nimbus"
      "--signer-workflow github.com/nimbus/nimbus/.github/workflows/release.yml"
      "--predicate-type https://slsa.dev/provenance/v1"
      "--source-ref refs/tags/v9.9.9"
      "--deny-self-hosted-runners"
      "--format json"
    )
    if [[ "${image_attestation}" -eq 1 ]]; then
      required_flags+=("--bundle-from-oci")
    fi
    for required in "${required_flags[@]}"; do
      case "${args}" in
        *"${required}"*) ;;
        *)
          echo "stub gh attestation verify missing ${required}: ${args}" >&2
          exit 1
          ;;
      esac
    done
    cat "${NIMBUS_LIVE_FIXTURE_ARTIFACTS:?}/nimbus_oci_attestation.json"
    ;;
  *)
    echo "unexpected gh invocation: $*" >&2
    exit 1
    ;;
esac
EOF
  chmod 0755 "${path}"
}

write_fake_nm() {
  local path="$1"
  cat >"${path}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
source "${repo_root}/scripts/bun-jsc-adapter-contract.sh"
for symbol in "\${BUN_JSC_ADAPTER_REQUIRED_EXPORTS[@]}"; do
  printf '0000000000000000 T %s\n' "\${symbol}"
done
EOF
  chmod 0755 "${path}"
}

create_release_fixture() {
  local dir="$1"
  local layout="${tmp_dir}/layout-$(basename "${dir}")"
  local multi_digest="sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  local amd64_digest="sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  local arm64_digest="sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"

  mkdir -p \
    "${dir}" \
    "${layout}/darwin" \
    "${layout}/darwin/libexec" \
    "${layout}/linux-x86_64" \
    "${layout}/linux-arm64" \
    "${layout}/windows"

  printf '#!/bin/sh\nprintf "nimbus 9.9.9\\n"\n' >"${layout}/darwin/nimbus"
  printf '#!/bin/sh\nprintf "nimbus 9.9.9\\n"\n' >"${layout}/linux-x86_64/nimbus"
  printf '#!/bin/sh\nprintf "nimbus 9.9.9\\n"\n' >"${layout}/linux-arm64/nimbus"
  printf 'windows fixture\n' >"${layout}/windows/nimbus.exe"
  # The shipped darwin archive bundles the pinned VMM helpers under libexec, and
  # verify-release-archive-layout.sh hard-requires both; mirror them here so the
  # live OCI fixture matches the real release layout.
  printf '#!/bin/sh\nprintf "gvproxy fixture\\n"\n' >"${layout}/darwin/libexec/gvproxy"
  printf '#!/bin/sh\nprintf "vfkit fixture\\n"\n' >"${layout}/darwin/libexec/vfkit"
  chmod 0755 \
    "${layout}/darwin/nimbus" \
    "${layout}/darwin/libexec/gvproxy" \
    "${layout}/darwin/libexec/vfkit" \
    "${layout}/linux-x86_64/nimbus" \
    "${layout}/linux-arm64/nimbus"

  for platform in darwin linux-x86_64 linux-arm64 windows; do
    printf 'Nimbus release fixture\n' >"${layout}/${platform}/README.md"
    cp "${repo_root}/LICENSE" "${layout}/${platform}/LICENSE"
  done

  tar -czf "${dir}/nimbus_darwin_arm64.tar.gz" \
    -C "${layout}/darwin" nimbus libexec README.md LICENSE
  tar -czf "${dir}/nimbus_linux_x86_64.tar.gz" \
    -C "${layout}/linux-x86_64" nimbus README.md LICENSE
  tar -czf "${dir}/nimbus_linux_arm64.tar.gz" \
    -C "${layout}/linux-arm64" nimbus README.md LICENSE
  (
    cd "${layout}/windows"
    zip -q "${dir}/nimbus_windows_x86_64.zip" nimbus.exe README.md LICENSE
  )

  cp "${repo_root}/LICENSE" "${dir}/LICENSE"
  printf '#!/bin/sh\nprintf "install fixture\\n"\n' >"${dir}/install.sh"
  chmod 0755 "${dir}/install.sh"

  adapter_target_triple="$(bun_jsc_adapter_host_triple)"
  adapter_platform_arch="$(bun_jsc_adapter_archive_platform_arch_for_triple "${adapter_target_triple}")"
  adapter_library_basename="$(bun_jsc_adapter_library_basename_for_triple "${adapter_target_triple}")"
  adapter_library="${tmp_dir}/live-fixture-${adapter_library_basename}"
  printf 'fixture shared adapter bytes\n' >"${adapter_library}"
  chmod 0755 "${adapter_library}"
  adapter_output="${tmp_dir}/adapter-package-$(basename "${dir}")"
  bash "${repo_root}/scripts/package-bun-jsc-adapter.sh" \
    --output-dir "${adapter_output}" \
    --shared-library "${adapter_library}" \
    --nimbus-version v9.9.9 \
    --adapter-version v9.9.9-bun-proof-main-20260525 \
    --target-triple "${adapter_target_triple}" \
    >"${tmp_dir}/package-adapter-$(basename "${dir}").out"
  cp \
    "${adapter_output}/nimbus-bun-jsc-adapter-${adapter_platform_arch}.tar.gz" \
    "${dir}/"
  printf 'optional regex-looking proof asset\n' >"${dir}/nimbus-extra+[proof].txt"

  bash "${repo_root}/scripts/render-release-oci-image-report.sh" \
    --image ghcr.io/nimbus/nimbus \
    --tag v9.9.9 \
    --multi-digest "${multi_digest}" \
    --amd64-digest "${amd64_digest}" \
    --arm64-digest "${arm64_digest}" \
    --latest-digest "${multi_digest}" \
    --output "${dir}/nimbus_oci_image.txt" \
    >"${tmp_dir}/render-live-fixture.out"

  cat >"${dir}/nimbus_oci_attestation.json" <<EOF
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

  cat >"${dir}/nimbus_oci_sbom.json" <<EOF
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

  cat >"${dir}/nimbus_oci_vulns.sarif.json" <<'EOF'
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

  (
    cd "${dir}"
    if command -v sha256sum >/dev/null 2>&1; then
      sha256sum \
        nimbus_darwin_arm64.tar.gz \
        nimbus_linux_arm64.tar.gz \
        nimbus_linux_x86_64.tar.gz \
        nimbus_windows_x86_64.zip \
        nimbus_oci_image.txt \
        nimbus_oci_attestation.json \
        nimbus_oci_sbom.json \
        nimbus_oci_vulns.sarif.json \
        nimbus-bun-jsc-adapter-*.tar.gz \
        'nimbus-extra+[proof].txt' \
        install.sh \
        LICENSE \
        > checksums-sha256.txt
    else
      shasum -a 256 \
        nimbus_darwin_arm64.tar.gz \
        nimbus_linux_arm64.tar.gz \
        nimbus_linux_x86_64.tar.gz \
        nimbus_windows_x86_64.zip \
        nimbus_oci_image.txt \
        nimbus_oci_attestation.json \
        nimbus_oci_sbom.json \
        nimbus_oci_vulns.sarif.json \
        nimbus-bun-jsc-adapter-*.tar.gz \
        'nimbus-extra+[proof].txt' \
        install.sh \
        LICENSE \
        > checksums-sha256.txt
    fi
  )
}

require_command jq
require_command tar
require_command zip
require_command unzip

fixture_dir="${tmp_dir}/release-assets"
missing_license_dir="${tmp_dir}/release-assets-missing-license"
unchecksummed_asset_dir="${tmp_dir}/release-assets-unchecksummed-extra"
stub_bin="${tmp_dir}/bin"
mkdir -p "${stub_bin}"
create_release_fixture "${fixture_dir}"
cp -R "${fixture_dir}" "${missing_license_dir}"
rm -f "${missing_license_dir}/LICENSE"
cp -R "${fixture_dir}" "${unchecksummed_asset_dir}"
printf 'unchecksummed release asset\n' >"${unchecksummed_asset_dir}/unexpected.txt"

write_stub_gh "${stub_bin}/gh"
write_fake_nm "${stub_bin}/nm"

good_output="${tmp_dir}/download-good"
mkdir -p "${good_output}"
good_output="$(cd "${good_output}" && pwd)"
NIMBUS_LIVE_FIXTURE_ARTIFACTS="${fixture_dir}" \
NIMBUS_LIVE_GH_LOG="${tmp_dir}/gh-good.log" \
NIMBUS_LIVE_DOWNLOAD_DIR="${good_output}" \
NM_BIN="${stub_bin}/nm" \
PATH="${stub_bin}:${PATH}" \
  bash "${repo_root}/scripts/verify-release-oci-image-live.sh" \
    --tag v9.9.9 \
    --output-dir "${good_output}" \
    --skip-smoke \
    >"${tmp_dir}/live-good.out"
grep -F "verified: nimbus/nimbus release" "${tmp_dir}/live-good.out" >/dev/null
grep -F "gh attestation verify" "${tmp_dir}/gh-good.log" >/dev/null
test -s "${good_output}/nimbus_oci_attestation.live.json"
test -s "${good_output}/nimbus_release_asset_attestations.live.jsonl"
asset_attestation_count="$(wc -l <"${good_output}/nimbus_release_asset_attestations.live.jsonl" | tr -d ' ')"
[[ "${asset_attestation_count}" == "13" ]] || die "expected 13 release asset attestations, got ${asset_attestation_count}"

bad_output="${tmp_dir}/download-missing-license"
mkdir -p "${bad_output}"
bad_output="$(cd "${bad_output}" && pwd)"
if NIMBUS_LIVE_FIXTURE_ARTIFACTS="${missing_license_dir}" \
  NIMBUS_LIVE_GH_LOG="${tmp_dir}/gh-bad.log" \
  NIMBUS_LIVE_DOWNLOAD_DIR="${bad_output}" \
  NM_BIN="${stub_bin}/nm" \
  PATH="${stub_bin}:${PATH}" \
    bash "${repo_root}/scripts/verify-release-oci-image-live.sh" \
      --tag v9.9.9 \
      --output-dir "${bad_output}" \
      --skip-smoke \
      >"${tmp_dir}/live-bad.out" 2>&1; then
  die "expected live verifier to reject a release without the top-level LICENSE asset"
fi
grep -F "expected exactly one LICENSE release asset, found 0" "${tmp_dir}/live-bad.out" >/dev/null

unchecksummed_output="${tmp_dir}/download-unchecksummed-extra"
mkdir -p "${unchecksummed_output}"
unchecksummed_output="$(cd "${unchecksummed_output}" && pwd)"
if NIMBUS_LIVE_FIXTURE_ARTIFACTS="${unchecksummed_asset_dir}" \
  NIMBUS_LIVE_GH_LOG="${tmp_dir}/gh-unchecksummed.log" \
  NIMBUS_LIVE_DOWNLOAD_DIR="${unchecksummed_output}" \
  NM_BIN="${stub_bin}/nm" \
  PATH="${stub_bin}:${PATH}" \
    bash "${repo_root}/scripts/verify-release-oci-image-live.sh" \
      --tag v9.9.9 \
      --output-dir "${unchecksummed_output}" \
      --skip-smoke \
      >"${tmp_dir}/live-unchecksummed.out" 2>&1; then
  die "expected live verifier to reject an unchecksummed release asset"
fi
grep -F "release asset is not covered by checksums-sha256.txt: unexpected.txt" \
  "${tmp_dir}/live-unchecksummed.out" >/dev/null

printf 'verified: live OCI image verifier accepts complete release fixtures with optional adapter assets and rejects missing LICENSE or unchecksummed release assets with stubbed GitHub evidence\n'
