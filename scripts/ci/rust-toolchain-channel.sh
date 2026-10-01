#!/usr/bin/env bash
# Print the `channel` that rust-toolchain.toml pins. dtolnay/rust-toolchain
# does not read that file, so CI passes this value as its `toolchain` input.
# Without it the action installs `stable`, and rustup then installs the pinned
# channel a second time without the job's extra components and targets.

set -euo pipefail

toolchain_file="${1:-rust-toolchain.toml}"

channel="$(tr -d '\r' <"${toolchain_file}" | sed -nE 's/^channel[[:space:]]*=[[:space:]]*"([^"]+)"[[:space:]]*$/\1/p')"
if [[ -z "${channel}" ]]; then
  printf '%s has no channel line\n' "${toolchain_file}" >&2
  exit 1
fi
printf '%s\n' "${channel}"
