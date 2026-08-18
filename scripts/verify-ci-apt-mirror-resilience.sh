#!/usr/bin/env bash
# Regression checks for the GitHub-hosted Ubuntu mirror failover used by the
# shared Rust setup action.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
helper="${repo_root}/scripts/ci/configure-ubuntu-apt-mirrors.sh"
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
printf '%s\n' \
  "${archive_mirror}" \
  "${azure_mirror}/" \
  "https://example.invalid/ubuntu/" > "${mixed}"
bash "${helper}" "${mixed}"
[[ "$(grep -Fxc "${archive_mirror}" "${mixed}")" == "1" ]] ||
  fail "canonical archive was not de-duplicated"
grep -Fxq 'https://example.invalid/ubuntu/' "${mixed}" ||
  fail "unrelated mirrors were not preserved"
if grep -Fq 'azure.archive.ubuntu.com' "${mixed}"; then
  fail "Azure regional mirror remained after failover"
fi

unchanged="${fixture_root}/unchanged.txt"
unchanged_expected="${fixture_root}/unchanged-expected.txt"
printf '%s\n' 'https://ports.ubuntu.com/ubuntu-ports/' > "${unchanged}"
cp "${unchanged}" "${unchanged_expected}"
bash "${helper}" "${unchanged}"
cmp -s "${unchanged}" "${unchanged_expected}" || fail "non-Azure mirror file changed"

missing="${fixture_root}/missing.txt"
bash "${helper}" "${missing}"
[[ ! -e "${missing}" ]] || fail "missing mirror file was created"

grep -Fq 'sudo bash scripts/ci/configure-ubuntu-apt-mirrors.sh /etc/apt/apt-mirrors.txt' "${action}" ||
  fail "shared Rust setup does not invoke the mirror failover"

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

printf 'PASS: CI Ubuntu mirror failover and bounded apt updates verified\n'
