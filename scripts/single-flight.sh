#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

usage() {
  cat <<'EOF'
Usage:
  bash scripts/single-flight.sh [--key <name>] -- <command> [args...]
  bash scripts/single-flight.sh --clear <name>

Examples:
  bash scripts/single-flight.sh --key cargo-test-workspace -- cargo test --workspace
  bash scripts/single-flight.sh -- cargo test -p neovex-engine
  bash scripts/single-flight.sh --clear cargo-test-workspace
EOF
}

default_key() {
  printf '%s\0' "$@" | cksum | awk '{print $1}'
}

key=""
clear_key=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --key)
      [[ $# -ge 2 ]] || {
        usage >&2
        exit 1
      }
      key="$2"
      shift 2
      ;;
    --clear)
      [[ $# -ge 2 ]] || {
        usage >&2
        exit 1
      }
      clear_key="$2"
      shift 2
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

lock_root="${NEOVEX_SINGLE_FLIGHT_DIR:-${SCRIPT_DIR}/../.neovex/single-flight}"
mkdir -p "$lock_root"

if [[ -n "$clear_key" ]]; then
  rm -rf "${lock_root}/${clear_key}"
  exit 0
fi

[[ $# -gt 0 ]] || {
  usage >&2
  exit 1
}

if [[ -z "$key" ]]; then
  key="$(default_key "$@")"
fi

lock_dir="${lock_root}/${key}"
pid_file="${lock_dir}/pid"
meta_file="${lock_dir}/meta"

command_display="$(printf '%q ' "$@")"

write_metadata() {
  printf '%s\n' "$$" >"$pid_file"
  cat >"$meta_file" <<EOF
cwd=$PWD
command=$command_display
started_at=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
EOF
}

cleanup_lock() {
  rm -rf "$lock_dir"
}

report_running_holder() {
  local holder_pid=""
  local holder_cwd=""
  local holder_command=""
  local holder_started_at=""

  if [[ -f "$pid_file" ]]; then
    holder_pid="$(cat "$pid_file" 2>/dev/null || true)"
  fi

  if [[ -f "$meta_file" ]]; then
    while IFS='=' read -r meta_key meta_value; do
      case "$meta_key" in
        cwd) holder_cwd="$meta_value" ;;
        command) holder_command="$meta_value" ;;
        started_at) holder_started_at="$meta_value" ;;
      esac
    done <"$meta_file"
  fi

  echo "single-flight: another wrapped verification command is already running" >&2
  echo "key: $key" >&2
  [[ -n "$holder_pid" ]] && echo "pid: $holder_pid" >&2
  [[ -n "$holder_started_at" ]] && echo "started_at: $holder_started_at" >&2
  [[ -n "$holder_cwd" ]] && echo "cwd: $holder_cwd" >&2
  [[ -n "$holder_command" ]] && echo "command: $holder_command" >&2
  echo "next_steps:" >&2
  echo "  1. Wait for the existing run to finish if it is still the run you want." >&2
  echo "  2. Only clear the lock if you are sure the earlier run is gone." >&2
  echo "  3. Clear a stale lock with: bash scripts/single-flight.sh --clear $key" >&2
}

acquire_lock() {
  if mkdir "$lock_dir" 2>/dev/null; then
    write_metadata
    trap cleanup_lock EXIT INT TERM
    return 0
  fi
  report_running_holder
  exit 75
}

acquire_lock
"$@"
