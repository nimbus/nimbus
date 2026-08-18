#!/usr/bin/env bash
# Replace the unreliable Azure regional Ubuntu mirror with the canonical
# archive mirror used by GitHub-hosted runner fallback. The caller owns
# privilege escalation; tests pass a writable fixture path.

set -euo pipefail

mirrors_file="${1:-/etc/apt/apt-mirrors.txt}"
archive_mirror="${NIMBUS_UBUNTU_ARCHIVE_MIRROR:-https://archive.ubuntu.com/ubuntu/}"

if [[ ! -f "${mirrors_file}" ]]; then
  exit 0
fi

if ! grep -Eq '^https?://azure\.archive\.ubuntu\.com/ubuntu/?$' "${mirrors_file}"; then
  exit 0
fi

tmp_file="$(mktemp "${mirrors_file}.tmp.XXXXXX")"
cleanup() {
  rm -f "${tmp_file}"
}
trap cleanup EXIT

awk -v replacement="${archive_mirror}" '
  $0 == replacement {
    if (!replacement_seen) {
      print
      replacement_seen = 1
    }
    next
  }
  /^https?:\/\/azure\.archive\.ubuntu\.com\/ubuntu\/?$/ {
    if (!replacement_seen) {
      print replacement
      replacement_seen = 1
    }
    next
  }
  { print }
' "${mirrors_file}" > "${tmp_file}"

chmod 0644 "${tmp_file}"
mv "${tmp_file}" "${mirrors_file}"
trap - EXIT
