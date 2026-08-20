#!/usr/bin/env bash
# Replace the unreliable Azure regional Ubuntu mirror with the canonical
# archive mirror used by GitHub-hosted runner fallback. The caller owns
# privilege escalation; tests pass a writable fixture path.

set -euo pipefail

mirrors_file="${1:-/etc/apt/apt-mirrors.txt}"
archive_mirror="https://archive.ubuntu.com/ubuntu/"

if [[ ! -f "${mirrors_file}" ]]; then
  exit 0
fi

azure_pattern='^https?://azure\.archive\.ubuntu\.com/ubuntu/?([[:space:]].*)?$'
archive_pattern='^https://archive\.ubuntu\.com/ubuntu/?([[:space:]].*)?$'

if ! grep -Eq "${azure_pattern}" "${mirrors_file}"; then
  exit 0
fi

archive_present=0
if grep -Eq "${archive_pattern}" "${mirrors_file}"; then
  archive_present=1
fi

tmp_file="$(mktemp "${mirrors_file}.tmp.XXXXXX")"
cleanup() {
  rm -f "${tmp_file}"
}
trap cleanup EXIT

awk -v archive_present="${archive_present}" -v replacement="${archive_mirror}" '
  /^https:\/\/archive\.ubuntu\.com\/ubuntu\/?([[:space:]].*)?$/ {
    if (!archive_seen) {
      print
      archive_seen = 1
    }
    next
  }
  /^https?:\/\/azure\.archive\.ubuntu\.com\/ubuntu\/?([[:space:]].*)?$/ {
    if (!archive_present && !archive_seen) {
      sub(/^https?:\/\/azure\.archive\.ubuntu\.com\/ubuntu\/?/, replacement)
      print
      archive_seen = 1
    }
    next
  }
  { print }
' "${mirrors_file}" > "${tmp_file}"

chmod 0644 "${tmp_file}"
mv "${tmp_file}" "${mirrors_file}"
trap - EXIT
