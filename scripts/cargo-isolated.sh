#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  bash scripts/cargo-isolated.sh [--root <dir>] -- <cargo-args...>

Examples:
  bash scripts/cargo-isolated.sh -- test -p neovex-engine queued_ -- --nocapture
  bash scripts/cargo-isolated.sh -- clippy -p neovex-engine --tests -- -D warnings

This wrapper gives the cargo invocation a unique CARGO_TARGET_DIR under /tmp
so a hung ad hoc test run does not block later focused cargo commands on the
shared artifact directory lock.
EOF
}

target_root="${NEOVEX_ISOLATED_CARGO_ROOT:-/tmp/neovex-cargo}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --root)
      [[ $# -ge 2 ]] || {
        usage >&2
        exit 1
      }
      target_root="$2"
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    --)
      shift
      break
      ;;
    *)
      usage >&2
      exit 1
      ;;
  esac
done

[[ $# -gt 0 ]] || {
  usage >&2
  exit 1
}

mkdir -p "$target_root"
target_dir="$(mktemp -d "${target_root%/}/run.XXXXXX")"
echo "cargo-isolated: CARGO_TARGET_DIR=$target_dir" >&2

exec env CARGO_TARGET_DIR="$target_dir" cargo "$@"
