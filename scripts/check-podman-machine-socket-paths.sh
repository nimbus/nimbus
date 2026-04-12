#!/usr/bin/env bash
set -euo pipefail

export LC_ALL=C

usage() {
  cat <<'EOF'
usage: check-podman-machine-socket-paths.sh --machine <name> [options]

Compute the Podman/libkrun unix-socket paths for a machine runtime directory
and report whether any derived path exceeds Darwin's sockaddr_un budget.

options:
  --machine <name>             Podman machine name to model (required)
  --tmp-root <path>            Machine runtime tmp root
                               (default: ${TMPDIR:-/tmp}/podman)
  --socket-byte-limit <bytes>  sockaddr_un.sun_path byte limit (default: 104)
  -h, --help                   Show this help

notes:
  - Darwin's sockaddr_un.sun_path holds 104 bytes including the trailing NUL.
  - The practical maximum path string length is therefore 103 characters.
  - Podman/libkrun currently needs at least these socket paths:
      <tmp-root>/<machine>.sock
      <tmp-root>/<machine>-api.sock
      <tmp-root>/<machine>-gvproxy.sock
      <tmp-root>/<machine>-gvproxy.sock-krun.sock
EOF
}

print_line() {
  local label="$1"
  local value="$2"
  printf '%-34s %s\n' "${label}" "${value}"
}

machine_name=""
tmp_root="${TMPDIR:-/tmp}/podman"
socket_byte_limit=104

while [[ $# -gt 0 ]]; do
  case "$1" in
    --machine)
      machine_name="${2:?missing machine name}"
      shift 2
      ;;
    --tmp-root)
      tmp_root="${2:?missing tmp root}"
      shift 2
      ;;
    --socket-byte-limit)
      socket_byte_limit="${2:?missing socket byte limit}"
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

if [[ -z "${machine_name}" ]]; then
  echo "missing required --machine argument" >&2
  usage >&2
  exit 64
fi

tmp_root="${tmp_root%/}"
max_path_chars=$((socket_byte_limit - 1))

ready_socket="${tmp_root}/${machine_name}.sock"
api_socket="${tmp_root}/${machine_name}-api.sock"
gvproxy_socket="${tmp_root}/${machine_name}-gvproxy.sock"
gvproxy_krun_socket="${gvproxy_socket}-krun.sock"

print_line "machine.name" "${machine_name}"
print_line "artifacts.tmp_root" "${tmp_root}"
print_line "darwin.sun_path_bytes" "${socket_byte_limit}"
print_line "darwin.max_path_chars" "${max_path_chars}"

path_labels=(
  "path.ready"
  "path.api"
  "path.gvproxy"
  "path.gvproxy_krun"
)
path_values=(
  "${ready_socket}"
  "${api_socket}"
  "${gvproxy_socket}"
  "${gvproxy_krun_socket}"
)

longest_label=""
longest_length=0
offending_labels=()

for index in "${!path_labels[@]}"; do
  label="${path_labels[${index}]}"
  value="${path_values[${index}]}"
  length="${#value}"

  print_line "${label}" "${value}"
  print_line "${label}.length" "${length}"

  if (( length > longest_length )); then
    longest_label="${label}"
    longest_length="${length}"
  fi

  if (( length > max_path_chars )); then
    offending_labels+=("${label}")
  fi
done

if (( ${#offending_labels[@]} == 0 )); then
  print_line "result" "ok offending=none longest=${longest_label} length=${longest_length} max_path_chars=${max_path_chars}"
  exit 0
fi

offending_joined="$(IFS=,; printf '%s' "${offending_labels[*]}")"
print_line "result" "too_long offending=${offending_joined} longest=${longest_label} length=${longest_length} max_path_chars=${max_path_chars}"
exit 0
