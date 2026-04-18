#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="$(mktemp -d "${TMPDIR:-/tmp}/neovex-release-archive-helper.XXXXXX")"
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
mkdir -p "${good_artifacts}/darwin/libexec" "${good_artifacts}/linux-x86_64" \
  "${good_artifacts}/linux-arm64" "${good_artifacts}/windows"

printf 'stub darwin host binary\n' > "${good_artifacts}/darwin/neovex"
printf 'stub gvproxy\n' > "${good_artifacts}/darwin/libexec/gvproxy"
printf 'stub linux amd64 binary\n' > "${good_artifacts}/linux-x86_64/neovex"
printf 'stub linux arm64 binary\n' > "${good_artifacts}/linux-arm64/neovex"
printf 'stub windows binary\n' > "${good_artifacts}/windows/neovex.exe"
printf 'readme\n' > "${good_artifacts}/darwin/README.md"
printf 'license\n' > "${good_artifacts}/darwin/LICENSE"
cp "${good_artifacts}/darwin/README.md" "${good_artifacts}/linux-x86_64/README.md"
cp "${good_artifacts}/darwin/LICENSE" "${good_artifacts}/linux-x86_64/LICENSE"
cp "${good_artifacts}/darwin/README.md" "${good_artifacts}/linux-arm64/README.md"
cp "${good_artifacts}/darwin/LICENSE" "${good_artifacts}/linux-arm64/LICENSE"
cp "${good_artifacts}/darwin/README.md" "${good_artifacts}/windows/README.md"
cp "${good_artifacts}/darwin/LICENSE" "${good_artifacts}/windows/LICENSE"

chmod 0755 "${good_artifacts}/darwin/neovex" \
  "${good_artifacts}/darwin/libexec/gvproxy" \
  "${good_artifacts}/linux-x86_64/neovex" \
  "${good_artifacts}/linux-arm64/neovex"

tar -czf "${good_artifacts}/neovex_darwin_arm64.tar.gz" \
  -C "${good_artifacts}/darwin" neovex libexec README.md LICENSE
tar -czf "${good_artifacts}/neovex_linux_x86_64.tar.gz" \
  -C "${good_artifacts}/linux-x86_64" neovex README.md LICENSE
tar -czf "${good_artifacts}/neovex_linux_arm64.tar.gz" \
  -C "${good_artifacts}/linux-arm64" neovex README.md LICENSE
(
  cd "${good_artifacts}/windows"
  zip -q "${good_artifacts}/neovex_windows_x86_64.zip" neovex.exe README.md LICENSE
)

bash "${repo_root}/scripts/verify-release-archive-layout.sh" \
  --artifacts-dir "${good_artifacts}" \
  > "${output_dir}/good.txt"
grep -F "verified: release archives match the published binary/layout contract" \
  "${output_dir}/good.txt" >/dev/null

cp -R "${good_artifacts}" "${bad_artifacts}"
rm -f "${bad_artifacts}/neovex_darwin_arm64.tar.gz"
rm -rf "${bad_artifacts}/darwin/libexec"
tar -czf "${bad_artifacts}/neovex_darwin_arm64.tar.gz" \
  -C "${bad_artifacts}/darwin" neovex README.md LICENSE

if bash "${repo_root}/scripts/verify-release-archive-layout.sh" \
  --artifacts-dir "${bad_artifacts}" \
  > "${output_dir}/bad.txt" 2>&1; then
  echo "expected release archive layout verification to fail when macOS gvproxy is missing" >&2
  exit 1
fi

grep -F "expected path missing: " "${output_dir}/bad.txt" >/dev/null
grep -F "libexec/gvproxy" "${output_dir}/bad.txt" >/dev/null

printf 'verified: release archive layout helper accepts the shipped layout and rejects a broken macOS helper bundle\n'
