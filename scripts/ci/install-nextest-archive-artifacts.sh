#!/usr/bin/env bash

set -euo pipefail

archive_dir="${1:?usage: install-nextest-archive-artifacts.sh <archive-dir> <nextest-config-dir>}"
config_dir="${2:?usage: install-nextest-archive-artifacts.sh <archive-dir> <nextest-config-dir>}"

mapfile -t archives < <(find "${archive_dir}" -type f -name 'nimbus-tests.tar.zst' | sort)
if [[ "${#archives[@]}" -ne 1 ]]; then
  printf 'expected exactly one nimbus-tests.tar.zst under %s, found %s\n' "${archive_dir}" "${#archives[@]}" >&2
  printf '%s\n' "${archives[@]}" >&2
  exit 1
fi
archive_file="${archives[0]}"

config_file="${config_dir}/.config/nextest.toml"
sha_file="${config_dir}/.config/nextest.toml.sha256"
if [[ ! -f "${config_file}" || ! -f "${sha_file}" ]]; then
  printf 'downloaded nextest-config artifact must contain .config/nextest.toml and .config/nextest.toml.sha256 under %s\n' "${config_dir}" >&2
  exit 1
fi

(
  cd "${config_dir}"
  sha256sum -c .config/nextest.toml.sha256
)

mkdir -p .config
cp "${config_file}" .config/nextest.toml

if [[ -n "${GITHUB_ENV:-}" ]]; then
  printf 'NIMBUS_TESTS_ARCHIVE=%s\n' "${archive_file}" >> "${GITHUB_ENV}"
fi

printf 'installed nextest config at .config/nextest.toml\n'
printf 'NIMBUS_TESTS_ARCHIVE=%s\n' "${archive_file}"
