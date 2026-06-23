#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-release-archive-helper.XXXXXX")"
trap 'rm -rf "${output_dir}"' EXIT

command -v tar >/dev/null 2>&1 || {
  echo "tar is required for release archive layout verification" >&2
  exit 1
}
command -v zip >/dev/null 2>&1 || {
  echo "zip is required for release archive layout verification" >&2
  exit 1
}
command -v unzip >/dev/null 2>&1 || {
  echo "unzip is required for release archive layout verification" >&2
  exit 1
}

good_artifacts="${output_dir}/good"
bad_artifacts="${output_dir}/bad"
bad_license_artifacts="${output_dir}/bad-license"
mkdir -p "${good_artifacts}/darwin" "${good_artifacts}/linux-x86_64" \
  "${good_artifacts}/linux-arm64" "${good_artifacts}/windows"

printf 'stub darwin host binary\n' > "${good_artifacts}/darwin/nimbus"
printf 'stub linux amd64 binary\n' > "${good_artifacts}/linux-x86_64/nimbus"
printf 'stub linux arm64 binary\n' > "${good_artifacts}/linux-arm64/nimbus"
printf 'stub windows binary\n' > "${good_artifacts}/windows/nimbus.exe"
printf 'readme\n' > "${good_artifacts}/darwin/README.md"
printf 'license\n' > "${good_artifacts}/darwin/LICENSE"
mkdir -p "${good_artifacts}/darwin/libexec"
printf 'stub gvproxy\n' > "${good_artifacts}/darwin/libexec/gvproxy"
printf 'stub vfkit\n' > "${good_artifacts}/darwin/libexec/vfkit"
cp "${good_artifacts}/darwin/README.md" "${good_artifacts}/linux-x86_64/README.md"
cp "${good_artifacts}/darwin/LICENSE" "${good_artifacts}/linux-x86_64/LICENSE"
cp "${good_artifacts}/darwin/README.md" "${good_artifacts}/linux-arm64/README.md"
cp "${good_artifacts}/darwin/LICENSE" "${good_artifacts}/linux-arm64/LICENSE"
cp "${good_artifacts}/darwin/README.md" "${good_artifacts}/windows/README.md"
cp "${good_artifacts}/darwin/LICENSE" "${good_artifacts}/windows/LICENSE"

chmod 0755 "${good_artifacts}/darwin/nimbus" \
  "${good_artifacts}/linux-x86_64/nimbus" \
  "${good_artifacts}/linux-arm64/nimbus" \
  "${good_artifacts}/darwin/libexec/gvproxy" \
  "${good_artifacts}/darwin/libexec/vfkit"

tar -czf "${good_artifacts}/nimbus_darwin_arm64.tar.gz" \
  -C "${good_artifacts}/darwin" nimbus libexec README.md LICENSE
tar -czf "${good_artifacts}/nimbus_linux_x86_64.tar.gz" \
  -C "${good_artifacts}/linux-x86_64" nimbus README.md LICENSE
tar -czf "${good_artifacts}/nimbus_linux_arm64.tar.gz" \
  -C "${good_artifacts}/linux-arm64" nimbus README.md LICENSE
(
  cd "${good_artifacts}/windows"
  zip -q "${good_artifacts}/nimbus_windows_x86_64.zip" nimbus.exe README.md LICENSE
)

bash "${repo_root}/scripts/verify-release-archive-layout.sh" \
  --artifacts-dir "${good_artifacts}" \
  > "${output_dir}/good.txt"
grep -F "verified: release archives match the published binary/layout contract" \
  "${output_dir}/good.txt" >/dev/null

cp -R "${good_artifacts}" "${bad_artifacts}"
rm -f "${bad_artifacts}/nimbus_darwin_arm64.tar.gz"
rm -rf "${bad_artifacts}/darwin/libexec"
tar -czf "${bad_artifacts}/nimbus_darwin_arm64.tar.gz" \
  -C "${bad_artifacts}/darwin" nimbus README.md LICENSE

if bash "${repo_root}/scripts/verify-release-archive-layout.sh" \
  --artifacts-dir "${bad_artifacts}" \
  > "${output_dir}/bad.txt" 2>&1; then
  echo "expected release archive layout verification to fail when macOS omits the bundled gvproxy helper" >&2
  exit 1
fi

grep -F "expected path missing: " "${output_dir}/bad.txt" >/dev/null
grep -F "libexec/gvproxy" "${output_dir}/bad.txt" >/dev/null

cp -R "${good_artifacts}" "${bad_license_artifacts}"
rm -f "${bad_license_artifacts}/nimbus_linux_x86_64.tar.gz"
rm -f "${bad_license_artifacts}/linux-x86_64/LICENSE"
tar -czf "${bad_license_artifacts}/nimbus_linux_x86_64.tar.gz" \
  -C "${bad_license_artifacts}/linux-x86_64" nimbus README.md

if bash "${repo_root}/scripts/verify-release-archive-layout.sh" \
  --artifacts-dir "${bad_license_artifacts}" \
  > "${output_dir}/bad-license.txt" 2>&1; then
  echo "expected release archive layout verification to fail when LICENSE is missing" >&2
  exit 1
fi

grep -F "expected path missing: " "${output_dir}/bad-license.txt" >/dev/null
grep -F "LICENSE" "${output_dir}/bad-license.txt" >/dev/null

bad_vfkit_artifacts="${output_dir}/bad-vfkit"
cp -R "${good_artifacts}" "${bad_vfkit_artifacts}"
rm -f "${bad_vfkit_artifacts}/nimbus_darwin_arm64.tar.gz"
rm -f "${bad_vfkit_artifacts}/darwin/libexec/vfkit"
tar -czf "${bad_vfkit_artifacts}/nimbus_darwin_arm64.tar.gz" \
  -C "${bad_vfkit_artifacts}/darwin" nimbus libexec README.md LICENSE

if bash "${repo_root}/scripts/verify-release-archive-layout.sh" \
  --artifacts-dir "${bad_vfkit_artifacts}" \
  > "${output_dir}/bad-vfkit.txt" 2>&1; then
  echo "expected release archive layout verification to fail when macOS omits the bundled vfkit helper" >&2
  exit 1
fi

grep -F "expected path missing: " "${output_dir}/bad-vfkit.txt" >/dev/null
grep -F "libexec/vfkit" "${output_dir}/bad-vfkit.txt" >/dev/null

printf 'verified: release archive layout helper accepts the bundled macOS gvproxy + vfkit layout and rejects a missing gvproxy helper, missing vfkit helper, or missing LICENSE payloads\n'
