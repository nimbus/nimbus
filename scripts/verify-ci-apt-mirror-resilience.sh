#!/usr/bin/env bash
# Regression checks for the GitHub-hosted Ubuntu mirror failover used by the
# shared Rust setup action.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
helper="${repo_root}/scripts/ci/configure-ubuntu-apt-mirrors.sh"
sources_helper="${repo_root}/scripts/ci/disable-unused-apt-sources.sh"
action="${repo_root}/.github/actions/setup-rust-cached/action.yml"
workflow="${repo_root}/.github/workflows/ci.yml"
fixture_root="$(mktemp -d)"

cleanup() {
  rm -rf "${fixture_root}"
}
trap cleanup EXIT

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

azure_mirror="http://azure.archive.ubuntu.com/ubuntu"
archive_mirror="https://archive.ubuntu.com/ubuntu/"

azure_only="${fixture_root}/azure-only.txt"
printf '%s\n' "${azure_mirror}" > "${azure_only}"
bash "${helper}" "${azure_only}"
[[ "$(cat "${azure_only}")" == "${archive_mirror}" ]] ||
  fail "Azure-only fixture was not replaced with the canonical archive"

mixed="${fixture_root}/mixed.txt"
printf '%s\t%s\n' "${azure_mirror}/" 'priority:1' > "${mixed}"
printf '%s\t%s\n' "${archive_mirror}" 'priority:2' >> "${mixed}"
printf '%s\n' "https://example.invalid/ubuntu/" >> "${mixed}"
bash "${helper}" "${mixed}"
[[ "$(grep -cF "${archive_mirror}" "${mixed}")" == "1" ]] ||
  fail "canonical archive was not de-duplicated"
grep -Fq $'https://archive.ubuntu.com/ubuntu/\tpriority:2' "${mixed}" ||
  fail "canonical archive priority attribute was not preserved"
grep -Fxq 'https://example.invalid/ubuntu/' "${mixed}" ||
  fail "unrelated mirrors were not preserved"
if grep -Fq 'azure.archive.ubuntu.com' "${mixed}"; then
  fail "Azure regional mirror remained after failover"
fi

attributed_only="${fixture_root}/attributed-only.txt"
printf '%s\t%s\n' "${azure_mirror}/" 'priority:1' > "${attributed_only}"
bash "${helper}" "${attributed_only}"
grep -Fq $'https://archive.ubuntu.com/ubuntu/\tpriority:1' "${attributed_only}" ||
  fail "replacement archive did not preserve the runner mirror priority attribute"

unchanged="${fixture_root}/unchanged.txt"
unchanged_expected="${fixture_root}/unchanged-expected.txt"
printf '%s\n' 'https://ports.ubuntu.com/ubuntu-ports/' > "${unchanged}"
cp "${unchanged}" "${unchanged_expected}"
bash "${helper}" "${unchanged}"
cmp -s "${unchanged}" "${unchanged_expected}" || fail "non-Azure mirror file changed"

missing="${fixture_root}/missing.txt"
bash "${helper}" "${missing}"
[[ ! -e "${missing}" ]] || fail "missing mirror file was created"

sources_dir="${fixture_root}/sources.list.d"
mkdir -p "${sources_dir}"
chrome_list="${sources_dir}/google-chrome.list"
printf '%s\n' '### THIS FILE IS AUTOMATICALLY CONFIGURED ###' \
  'deb [arch=amd64] https://dl.google.com/linux/chrome-stable/deb/ stable main' > "${chrome_list}"
chrome_sources="${sources_dir}/google-chrome.sources"
printf '%s\n' 'Types: deb' 'URIs: https://dl.google.com/linux/chrome-stable/deb/' \
  'Suites: stable' 'Components: main' > "${chrome_sources}"
microsoft_list="${sources_dir}/microsoft-prod.list"
printf '%s\n' 'deb [arch=amd64,arm64] https://packages.microsoft.com/ubuntu/24.04/prod noble main' > "${microsoft_list}"
ubuntu_sources="${sources_dir}/ubuntu.sources"
printf '%s\n' 'Types: deb' 'URIs: mirror+file:/etc/apt/apt-mirrors.txt' \
  'Suites: noble noble-updates noble-backports' 'Components: main universe restricted multiverse' > "${ubuntu_sources}"
ubuntu_sources_expected="${fixture_root}/ubuntu.sources.expected"
cp "${ubuntu_sources}" "${ubuntu_sources_expected}"
bash "${sources_helper}" "${sources_dir}"
[[ ! -e "${chrome_list}" ]] || fail "Google Chrome .list apt source was not removed"
[[ ! -e "${chrome_sources}" ]] || fail "Google Chrome deb822 apt source was not removed"
[[ -e "${microsoft_list}" ]] || fail "unrelated third-party apt source was removed"
cmp -s "${ubuntu_sources}" "${ubuntu_sources_expected}" || fail "Ubuntu apt source changed"

missing_sources_dir="${fixture_root}/missing.sources.list.d"
bash "${sources_helper}" "${missing_sources_dir}"
[[ ! -e "${missing_sources_dir}" ]] || fail "missing apt sources directory was created"

grep -Fq 'sudo bash scripts/ci/configure-ubuntu-apt-mirrors.sh /etc/apt/apt-mirrors.txt' "${action}" ||
  fail "shared Rust setup does not invoke the mirror failover"

grep -Fq 'sudo bash scripts/ci/disable-unused-apt-sources.sh /etc/apt/sources.list.d' "${action}" ||
  fail "shared Rust setup does not remove unused third-party apt sources"

for apt_setting in \
  'Acquire::Retries "3";' \
  'Acquire::http::Timeout "20";' \
  'Acquire::https::Timeout "20";'; do
  grep -Fq "${apt_setting}" "${action}" ||
    fail "shared Rust setup is missing apt bound: ${apt_setting}"
done

grep -Fq '/etc/apt/apt.conf.d/99-nimbus-network-bounds' "${action}" ||
  fail "shared Rust setup does not persist apt network bounds for later steps"

grep -Fq 'bash scripts/verify-ci-apt-mirror-resilience.sh' "${workflow}" ||
  fail "required CI does not execute the mirror regression helper"

printf 'PASS: CI Ubuntu mirror failover, unused apt source removal, and bounded apt updates verified\n'
