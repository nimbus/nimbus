#!/usr/bin/env bash
# Remove third-party apt sources that GitHub-hosted runner images ship but
# that CI never installs from. The Google Chrome repository on dl.google.com
# intermittently serves a Packages index whose checksum does not match its
# InRelease file. apt-get update then exits 100 for every job, even though no
# job installs a package from that repository. The caller owns privilege
# escalation; tests pass a writable fixture directory.

set -euo pipefail

sources_dir="${1:-/etc/apt/sources.list.d}"
unused_host_pattern='dl\.google\.com'

if [[ ! -d "${sources_dir}" ]]; then
  exit 0
fi

shopt -s nullglob
for source_file in "${sources_dir}"/*.list "${sources_dir}"/*.sources; do
  if grep -Eq "${unused_host_pattern}" "${source_file}"; then
    rm -f "${source_file}"
    printf 'removed unused apt source %s\n' "${source_file}"
  fi
done
