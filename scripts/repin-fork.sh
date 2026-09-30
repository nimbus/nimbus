#!/usr/bin/env bash
# Moves one Nimbus-owned fork to a new release tag in packaging/forks.toml and
# in every consumer file that the manifest lists.

set -euo pipefail

usage() {
  cat <<'EOF'
usage: repin-fork.sh FORK TAG [--commit SHA]

Rewrite the pin of FORK (deno, rusty_v8, bun, libkrun, or crun) to TAG in
packaging/forks.toml and in every consumer file, then verify the consumers.
Running it with the current tag changes no file.

Options:
  --commit SHA  The commit that TAG peels to. Forks whose consumers pin a
                commit resolve it with git ls-remote when this is omitted.
EOF
}

if [[ $# -eq 1 && ( "$1" == "-h" || "$1" == "--help" ) ]]; then
  usage
  exit 0
fi

if [[ $# -ne 2 && $# -ne 4 ]] || [[ $# -eq 4 && "$3" != "--commit" ]]; then
  usage >&2
  exit 64
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 "${repo_root}/scripts/fork_pins.py" repin "$@"
