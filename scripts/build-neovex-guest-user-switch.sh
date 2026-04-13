#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "build-neovex-guest-user-switch.sh requires Linux" >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_root="${1:-/tmp/neovex-guest-user-switch-root}"
binary_name="neovex-guest-user-switch"

cd "$repo_root"

cargo rustc -p neovex-sandbox --bin "$binary_name" --release -- -C target-feature=+crt-static

binary_path="$repo_root/target/release/$binary_name"
if [[ ! -x "$binary_path" ]]; then
  echo "expected helper binary at $binary_path" >&2
  exit 1
fi

mkdir -p "$output_root"
install -m 0755 "$binary_path" "$output_root/$binary_name"

if ldd "$output_root/$binary_name" 2>&1 | grep -q "not a dynamic executable"; then
  printf '%s\n' "$output_root"
  exit 0
fi

echo "expected a static guest helper binary, but ldd reported dynamic dependencies" >&2
exit 1
