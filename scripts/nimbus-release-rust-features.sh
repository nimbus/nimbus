#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/nimbus-release-rust-features.sh [--target <triple>] [--format cargo-args|feature-list]

Print the Rust feature selection for a Nimbus-owned release binary target.
If --target is omitted, the local rustc host triple is used.
USAGE
}

target=""
format="cargo-args"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --target)
      if [ "$#" -lt 2 ]; then
        echo "--target requires a Rust target triple" >&2
        exit 64
      fi
      target="$2"
      shift 2
      ;;
    --format)
      if [ "$#" -lt 2 ]; then
        echo "--format requires cargo-args or feature-list" >&2
        exit 64
      fi
      format="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 64
      ;;
  esac
done

if [ -z "${target}" ]; then
  target="$(rustc -vV | sed -n 's/^host: //p')"
fi

case "${format}" in
  cargo-args|feature-list)
    ;;
  *)
    echo "unsupported format: ${format}" >&2
    usage >&2
    exit 64
    ;;
esac

# Keep the crate feature explicit and target-bounded. These are the Nimbus
# release targets with published ptrcomp+simdutf rusty_v8 assets today.
case "${target}" in
  x86_64-unknown-linux-gnu|aarch64-apple-darwin)
    if [ "${format}" = "cargo-args" ]; then
      printf '%s\n' '--features v8-pointer-compression'
    else
      printf '%s\n' 'v8-pointer-compression'
    fi
    ;;
  aarch64-unknown-linux-gnu|x86_64-pc-windows-msvc|*)
    ;;
esac
