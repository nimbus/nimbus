#!/usr/bin/env bash
# Verifies that every copy of a Nimbus-owned fork pin matches
# packaging/forks.toml, and optionally inventories the fork checkouts.

set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-fork-upstream-standardization.sh [--remote] [--fork NAME]...

Check every consumer file in packaging/forks.toml against the pinned fork tag,
upstream version, and commit. Fail when a consumer drifts, or when a tracked
file copies a fork tag or commit without a consumer entry. This check is
offline and deterministic.

Options:
  --remote  Also inventory the local fork checkouts under ~/src/github.com and
            their git remotes, and report newer upstream tags. Needs network.
  --fork    Verify only the named fork (deno, rusty_v8, bun, libkrun, crun, or
            OWNER/REPO). Repeat to select multiple forks.
EOF
}

remote=0
fork_args=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --remote)
      remote=1
      shift
      ;;
    --fork)
      if [[ $# -lt 2 || -z "$2" ]]; then
        printf '%s\n' '--fork requires a fork name' >&2
        usage >&2
        exit 64
      fi
      fork_args+=(--fork "$2")
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 64
      ;;
  esac
done

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fork_pins=(python3 "${repo_root}/scripts/fork_pins.py")

status=0
"${fork_pins[@]}" check "${fork_args[@]+"${fork_args[@]}"}" || status=1

if [[ "${remote}" -eq 1 ]]; then
  "${fork_pins[@]}" inventory "${fork_args[@]+"${fork_args[@]}"}" || status=1
fi

exit "${status}"
