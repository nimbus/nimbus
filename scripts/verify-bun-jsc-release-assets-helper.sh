#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "${repo_root}/scripts/bun-jsc-adapter-contract.sh"

tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-bun-jsc-release-assets-helper.XXXXXX")"
trap 'rm -rf "${tmp_root}"' EXIT

target_triple="$(bun_jsc_adapter_host_triple)"
platform_arch="$(bun_jsc_adapter_archive_platform_arch_for_triple "${target_triple}")"
library_basename="$(bun_jsc_adapter_library_basename_for_triple "${target_triple}")"
fixture_library="${tmp_root}/${library_basename}"
printf 'fixture shared adapter bytes\n' >"${fixture_library}"
chmod 0755 "${fixture_library}"

fake_nm="${tmp_root}/fake-nm"
cat >"${fake_nm}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
source "${repo_root}/scripts/bun-jsc-adapter-contract.sh"
for symbol in "\${BUN_JSC_ADAPTER_REQUIRED_EXPORTS[@]}"; do
  printf '0000000000000000 T %s\n' "\${symbol}"
done
EOF
chmod 0755 "${fake_nm}"

empty_assets="${tmp_root}/empty-assets"
mkdir -p "${empty_assets}"
bash "${repo_root}/scripts/verify-bun-jsc-release-assets.sh" \
  --artifacts-dir "${empty_assets}" \
  >"${tmp_root}/empty.out"
grep -F "optional Bun/JSC adapter release assets are absent by policy" \
  "${tmp_root}/empty.out" >/dev/null

if bash "${repo_root}/scripts/verify-bun-jsc-release-assets.sh" \
  --artifacts-dir "${empty_assets}" \
  --require-platform "${platform_arch}" \
  >"${tmp_root}/missing-required.out" 2>&1; then
  printf 'expected missing required adapter platform to fail\n' >&2
  exit 1
fi
grep -F "required Bun/JSC adapter archive is missing" \
  "${tmp_root}/missing-required.out" >/dev/null

package_output="${tmp_root}/package"
bash "${repo_root}/scripts/package-bun-jsc-adapter.sh" \
  --output-dir "${package_output}" \
  --shared-library "${fixture_library}" \
  --nimbus-version v0.1.0 \
  --adapter-version v0.1.0-bun-proof-main-20260525 \
  --target-triple "${target_triple}" \
  >"${tmp_root}/package.out"

archive_path="$(awk -F= '$1 == "archive.path" { print $2 }' "${tmp_root}/package.out")"
[[ -f "${archive_path}" ]] || {
  printf 'package helper did not create archive: %s\n' "${archive_path}" >&2
  exit 1
}

assets_dir="${tmp_root}/assets"
mkdir -p "${assets_dir}"
cp "${archive_path}" "${assets_dir}/"
checksums_path="${assets_dir}/nimbus-bun-jsc-adapter-checksums-sha256.txt"
(
  cd "${assets_dir}"
  sha256="$(bun_jsc_adapter_sha256_file "nimbus-bun-jsc-adapter-${platform_arch}.tar.gz")"
  printf '%s  nimbus-bun-jsc-adapter-%s.tar.gz\n' "${sha256}" "${platform_arch}" \
    >"${checksums_path}"
)

bash "${repo_root}/scripts/verify-bun-jsc-release-assets.sh" \
  --artifacts-dir "${assets_dir}" \
  --checksums "${checksums_path}" \
  --require-platform "${platform_arch}" \
  --nm "${fake_nm}" \
  >"${tmp_root}/good.out"
grep -F "optional Bun/JSC adapter release assets match package and checksum contracts" \
  "${tmp_root}/good.out" >/dev/null

bad_checksums="${tmp_root}/bad-checksums.txt"
printf '%064d  nimbus-bun-jsc-adapter-%s.tar.gz\n' 0 "${platform_arch}" >"${bad_checksums}"
if bash "${repo_root}/scripts/verify-bun-jsc-release-assets.sh" \
  --artifacts-dir "${assets_dir}" \
  --checksums "${bad_checksums}" \
  --require-platform "${platform_arch}" \
  --nm "${fake_nm}" \
  >"${tmp_root}/bad-checksums.out" 2>&1; then
  printf 'expected bad release checksums to fail\n' >&2
  exit 1
fi
grep -F "checksums file does not contain matching" \
  "${tmp_root}/bad-checksums.out" >/dev/null

bad_subject_checksums="${tmp_root}/bad-subject-checksums.txt"
(
  cd "${assets_dir}"
  sha256="$(bun_jsc_adapter_sha256_file "nimbus-bun-jsc-adapter-${platform_arch}.tar.gz")"
  printf '%s  nimbus-bun-jsc-adapter-%s.tar.gz.evil\n' "${sha256}" "${platform_arch}" \
    >"${bad_subject_checksums}"
)
if bash "${repo_root}/scripts/verify-bun-jsc-release-assets.sh" \
  --artifacts-dir "${assets_dir}" \
  --checksums "${bad_subject_checksums}" \
  --require-platform "${platform_arch}" \
  --nm "${fake_nm}" \
  >"${tmp_root}/bad-subject-checksums.out" 2>&1; then
  printf 'expected bad release checksum subject to fail\n' >&2
  exit 1
fi
grep -F "checksums file does not contain matching" \
  "${tmp_root}/bad-subject-checksums.out" >/dev/null

unknown_assets="${tmp_root}/unknown-assets"
mkdir -p "${unknown_assets}"
cp "${archive_path}" "${unknown_assets}/nimbus-bun-jsc-adapter-plan9-x86_64.tar.gz"
if bash "${repo_root}/scripts/verify-bun-jsc-release-assets.sh" \
  --artifacts-dir "${unknown_assets}" \
  --nm "${fake_nm}" \
  >"${tmp_root}/unknown.out" 2>&1; then
  printf 'expected unknown adapter platform to fail\n' >&2
  exit 1
fi
grep -F "unsupported Bun/JSC adapter release asset platform" \
  "${tmp_root}/unknown.out" >/dev/null

tampered_assets="${tmp_root}/tampered-assets"
tampered_extract="${tmp_root}/tampered-extract"
mkdir -p "${tampered_assets}" "${tampered_extract}"
tar -xzf "${archive_path}" -C "${tampered_extract}"
printf 'tamper\n' >>"${tampered_extract}/${library_basename}"
(
  cd "${tampered_extract}"
  tar -czf "${tampered_assets}/nimbus-bun-jsc-adapter-${platform_arch}.tar.gz" \
    $(find . -maxdepth 1 -type f -print | sed 's#^\./##' | sort)
)
tampered_checksums="${tampered_assets}/nimbus-bun-jsc-adapter-checksums-sha256.txt"
(
  cd "${tampered_assets}"
  sha256="$(bun_jsc_adapter_sha256_file "nimbus-bun-jsc-adapter-${platform_arch}.tar.gz")"
  printf '%s  nimbus-bun-jsc-adapter-%s.tar.gz\n' "${sha256}" "${platform_arch}" \
    >"${tampered_checksums}"
)
if bash "${repo_root}/scripts/verify-bun-jsc-release-assets.sh" \
  --artifacts-dir "${tampered_assets}" \
  --checksums "${tampered_checksums}" \
  --require-platform "${platform_arch}" \
  --nm "${fake_nm}" \
  >"${tmp_root}/tampered.out" 2>&1; then
  printf 'expected tampered adapter package to fail\n' >&2
  exit 1
fi
grep -F "checksums file does not contain matching ${library_basename} digest" \
  "${tmp_root}/tampered.out" >/dev/null

printf 'verified: Bun/JSC release asset helper accepts absent-optional and good assets with SBOM/provenance, rejects missing required assets, bad release checksums, checksum subject spoofing, unknown platforms, and tampered adapter packages\n'
