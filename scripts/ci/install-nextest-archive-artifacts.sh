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

# upload-artifact strips the common ancestor (.config/), so the downloaded
# artifact usually carries nextest.toml at its ROOT; accept both layouts.
config_file="${config_dir}/.config/nextest.toml"
sha_file="${config_dir}/.config/nextest.toml.sha256"
if [[ ! -f "${config_file}" && -f "${config_dir}/nextest.toml" ]]; then
  config_file="${config_dir}/nextest.toml"
  sha_file="${config_dir}/nextest.toml.sha256"
fi
if [[ ! -f "${config_file}" || ! -f "${sha_file}" ]]; then
  printf 'downloaded nextest-config artifact must contain .config/nextest.toml and .config/nextest.toml.sha256 under %s\n' "${config_dir}" >&2
  exit 1
fi

# Install first, then verify at the repo root: the sidecar's embedded name is
# ".config/nextest.toml" (B4 generates it from the repo root), which only
# resolves after installation regardless of the artifact's internal layout.
mkdir -p .config
cp "${config_file}" .config/nextest.toml
cp "${sha_file}" .config/nextest.toml.sha256
sha256sum -c .config/nextest.toml.sha256

# Deno extension sources: archived binaries read lazy-loaded JS from ABSOLUTE
# builder paths under ~/.cargo/git/checkouts/deno-*/. The archive artifact
# ships those sources (deno-src-checkout.tgz, filtered to ext/+libs JS/TS);
# restore them at the exact baked location. FAIL CLOSED: without them every
# JsRuntime construction panics ENOENT.
deno_tgz="${archive_dir}/deno-src-checkout.tgz"
if [[ ! -f "${deno_tgz}" || ! -f "${deno_tgz}.sha256" ]]; then
  printf 'archive artifact is missing deno-src-checkout.tgz(+.sha256) — V8 tests cannot run\n' >&2
  exit 1
fi
(
  cd "${archive_dir}"
  sha256sum -c deno-src-checkout.tgz.sha256
)
mkdir -p "${HOME}/.cargo/git/checkouts"
tar -xzf "${deno_tgz}" -C "${HOME}/.cargo/git/checkouts"
printf 'restored deno extension sources under %s\n' "${HOME}/.cargo/git/checkouts"

if [[ -n "${GITHUB_ENV:-}" ]]; then
  printf 'NIMBUS_TESTS_ARCHIVE=%s\n' "${archive_file}" >> "${GITHUB_ENV}"
fi

printf 'installed nextest config at .config/nextest.toml\n'
printf 'NIMBUS_TESTS_ARCHIVE=%s\n' "${archive_file}"
