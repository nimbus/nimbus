#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-runtime-separation.sh [options]

Verify that the system OCI runtime remains separate from the private nimbus
runtime path. The helper records system-runtime, private-runtime, and Podman
runtime evidence and exits non-zero when the paths collapse onto the same
runtime.

options:
  --system-runtime <path-or-command>   System runtime command (default: crun)
  --private-runtime <path>             Private nimbus runtime path (default: /usr/libexec/nimbus/crun)
  --podman <path-or-command>           Podman command (default: podman)
  -h, --help                           Show this help

examples:
  bash scripts/verify-runtime-separation.sh

  bash scripts/verify-runtime-separation.sh \
    --system-runtime /usr/bin/crun \
    --private-runtime /usr/libexec/nimbus/crun
EOF
}

print_line() {
  printf '%-28s %s\n' "$1" "$2"
}

compact_value() {
  printf '%s' "$1" | tr '\n' ' ' | sed -e 's/[[:space:]]\+/ /g' -e 's/^ //' -e 's/ $//'
}

resolve_command_path() {
  local target="$1"

  if [[ "${target}" == */* ]]; then
    if [[ -x "${target}" ]]; then
      printf '%s' "${target}"
      return 0
    fi
    return 1
  fi

  command -v "${target}" 2>/dev/null
}

version_line() {
  local target="$1"
  local line=""

  line="$("${target}" --version 2>/dev/null | head -n1 || true)"
  printf '%s' "${line}"
}

real_path_or_self() {
  local path="$1"

  python3 - "$path" <<'PY'
import os
import sys

path = sys.argv[1]
print(os.path.realpath(path))
PY
}

system_runtime="crun"
private_runtime="/usr/libexec/nimbus/crun"
podman_command="podman"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --system-runtime)
      system_runtime="${2:-}"
      shift 2
      ;;
    --private-runtime)
      private_runtime="${2:-}"
      shift 2
      ;;
    --podman)
      podman_command="${2:-}"
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

failures=0

system_runtime_path="$(resolve_command_path "${system_runtime}" || true)"
private_runtime_path="$(resolve_command_path "${private_runtime}" || true)"
podman_path="$(resolve_command_path "${podman_command}" || true)"

if [[ -n "${system_runtime_path}" ]]; then
  print_line "system.runtime.path" "${system_runtime_path}"
  system_version="$(version_line "${system_runtime_path}")"
  if [[ -n "${system_version}" ]]; then
    print_line "system.runtime.version" "${system_version}"
  fi
else
  print_line "system.runtime.path" "missing"
  failures=$((failures + 1))
fi

if [[ -n "${private_runtime_path}" ]]; then
  print_line "private.runtime.path" "${private_runtime_path}"
  private_version="$(version_line "${private_runtime_path}")"
  if [[ -n "${private_version}" ]]; then
    print_line "private.runtime.version" "${private_version}"
  fi
else
  print_line "private.runtime.path" "missing"
  failures=$((failures + 1))
fi

podman_runtime=""
podman_runtime_path=""
if [[ -n "${podman_path}" ]]; then
  print_line "podman.path" "${podman_path}"
  podman_version="$(version_line "${podman_path}")"
  if [[ -n "${podman_version}" ]]; then
    print_line "podman.version" "${podman_version}"
  fi
  podman_runtime="$("${podman_path}" info --format '{{.Host.OCIRuntime.Name}} {{.Host.OCIRuntime.Path}}' 2>/dev/null || true)"
  if [[ -n "${podman_runtime}" ]]; then
    podman_runtime_compact="$(compact_value "${podman_runtime}")"
    print_line "podman.runtime" "${podman_runtime_compact}"
    podman_runtime_path="$(printf '%s\n' "${podman_runtime_compact}" | awk '{print $NF}')"
    if [[ -n "${podman_runtime_path}" ]]; then
      print_line "podman.runtime.path" "${podman_runtime_path}"
    fi
  else
    print_line "podman.runtime" "unavailable"
    failures=$((failures + 1))
  fi
else
  print_line "podman.path" "missing"
  failures=$((failures + 1))
fi

if [[ -n "${system_runtime_path}" && -n "${private_runtime_path}" ]]; then
  system_runtime_real="$(real_path_or_self "${system_runtime_path}")"
  private_runtime_real="$(real_path_or_self "${private_runtime_path}")"
  print_line "system.runtime.realpath" "${system_runtime_real}"
  print_line "private.runtime.realpath" "${private_runtime_real}"

  if [[ "${system_runtime_real}" == "${private_runtime_real}" ]]; then
    print_line "runtime.separation" "failed (system and private runtimes resolve to the same path)"
    failures=$((failures + 1))
  else
    print_line "runtime.separation" "ok"
  fi
fi

if [[ -n "${podman_runtime}" && -n "${private_runtime_path}" ]]; then
  private_runtime_real="${private_runtime_real:-$(real_path_or_self "${private_runtime_path}")}"
  podman_runtime_compact="${podman_runtime_compact:-$(compact_value "${podman_runtime}")}"
  podman_runtime_resolved=""
  podman_runtime_real=""

  if [[ -n "${podman_runtime_path}" ]]; then
    podman_runtime_resolved="$(resolve_command_path "${podman_runtime_path}" || true)"
    if [[ -n "${podman_runtime_resolved}" ]]; then
      podman_runtime_real="$(real_path_or_self "${podman_runtime_resolved}")"
      print_line "podman.runtime.realpath" "${podman_runtime_real}"
    fi
  fi

  if [[ "${podman_runtime_compact}" == *"${private_runtime_path}"* ]] || \
     [[ "${podman_runtime_compact}" == *"${private_runtime_real}"* ]] || \
     [[ -n "${podman_runtime_resolved}" && "${podman_runtime_resolved}" == "${private_runtime_path}" ]] || \
     [[ -n "${podman_runtime_real}" && "${podman_runtime_real}" == "${private_runtime_real}" ]]; then
    print_line "podman.runtime.separation" "failed (Podman points at the private nimbus runtime)"
    failures=$((failures + 1))
  else
    print_line "podman.runtime.separation" "ok"
  fi
fi

if [[ "${failures}" -eq 0 ]]; then
  print_line "result" "separate"
  exit 0
fi

print_line "result" "not-separate (${failures} failing checks)"
exit 1
