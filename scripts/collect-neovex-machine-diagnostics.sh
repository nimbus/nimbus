#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: collect-neovex-machine-diagnostics.sh [options]

Collect best-effort diagnostics for a Neovex macOS machine-manager state root.
This captures the persisted config/state records plus the short runtime-root
socket, pid, and log inventory that MAC3 owns.

options:
  --machine <name>             Machine name (default: default)
  --home <path>                HOME to use when deriving XDG-style roots
  --config-root <path>         Override config root
  --state-root <path>          Override state root
  --runtime-root <path>        Override runtime root
  --output-dir <path>          Output directory for captured artifacts
  --neovex <path>              Optional neovex binary for `machine status`
  --ps <path>                  Override ps command path
  --log-lines <count>          Number of log lines to copy from each log
  -h, --help                   Show this help

examples:
  bash scripts/collect-neovex-machine-diagnostics.sh \
    --home /tmp/neovex-home \
    --runtime-root /tmp/neovex \
    --output-dir /tmp/neovex-machine-diag
EOF
}

print_line() {
  local label="$1"
  local value="$2"
  printf '%-34s %s\n' "${label}" "${value}" | tee -a "${summary_file}"
}

capture_command() {
  local label="$1"
  local output_path="$2"
  shift 2

  local status=0

  set +e
  "$@" >"${output_path}" 2>&1
  status=$?
  set -e

  if [[ "${status}" -eq 0 ]]; then
    print_line "${label}" "ok path=${output_path}"
  else
    print_line "${label}" "failed status=${status} path=${output_path}"
  fi

  return 0
}

capture_processes() {
  local output_path="$1"
  local status=0
  local mode="structured"

  set +e
  "${ps_bin}" -ax -o pid=,ppid=,command= >"${output_path}" 2>&1
  status=$?
  if [[ "${status}" -ne 0 ]]; then
    mode="plain"
    "${ps_bin}" -ax >"${output_path}" 2>&1
    status=$?
  fi
  set -e

  if [[ "${status}" -eq 0 ]]; then
    print_line "capture.processes" "ok path=${output_path} mode=${mode}"
  else
    print_line "capture.processes" "failed status=${status} path=${output_path}"
  fi

  return 0
}

copy_if_present() {
  local label="$1"
  local source_path="$2"
  local output_path="$3"

  if [[ -f "${source_path}" ]]; then
    cp "${source_path}" "${output_path}"
    print_line "${label}" "present source=${source_path} copy=${output_path}"
  else
    print_line "${label}" "missing path=${source_path}"
  fi
}

list_dir_if_present() {
  local label="$1"
  local dir_path="$2"
  local output_path="$3"

  if [[ -d "${dir_path}" ]]; then
    ls -la "${dir_path}" >"${output_path}"
    print_line "${label}" "present path=${dir_path} listing=${output_path}"
  else
    print_line "${label}" "missing path=${dir_path}"
  fi
}

tail_file_if_present() {
  local label="$1"
  local source_path="$2"
  local output_path="$3"
  local line_count="$4"

  if [[ -f "${source_path}" ]]; then
    tail -n "${line_count}" "${source_path}" >"${output_path}"
    print_line "${label}" "present path=${source_path} tail=${output_path}"
  else
    print_line "${label}" "missing path=${source_path}"
  fi
}

machine_name="default"
home_dir="${HOME:-}"
config_root=""
state_root=""
runtime_root="${NEOVEX_MACHINE_RUNTIME_ROOT:-/tmp/neovex}"
output_dir=""
neovex_bin=""
ps_bin="ps"
log_lines=120

while [[ $# -gt 0 ]]; do
  case "$1" in
    --machine)
      machine_name="${2:?missing machine name}"
      shift 2
      ;;
    --home)
      home_dir="${2:?missing home path}"
      shift 2
      ;;
    --config-root)
      config_root="${2:?missing config root}"
      shift 2
      ;;
    --state-root)
      state_root="${2:?missing state root}"
      shift 2
      ;;
    --runtime-root)
      runtime_root="${2:?missing runtime root}"
      shift 2
      ;;
    --output-dir)
      output_dir="${2:?missing output dir}"
      shift 2
      ;;
    --neovex)
      neovex_bin="${2:?missing neovex path}"
      shift 2
      ;;
    --ps)
      ps_bin="${2:?missing ps path}"
      shift 2
      ;;
    --log-lines)
      log_lines="${2:?missing log line count}"
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

if [[ -z "${config_root}" ]]; then
  if [[ -n "${XDG_CONFIG_HOME:-}" ]]; then
    config_root="${XDG_CONFIG_HOME%/}/neovex/machine"
  elif [[ -n "${home_dir}" ]]; then
    config_root="${home_dir%/}/.config/neovex/machine"
  else
    echo "set --config-root or provide --home/HOME so the config root can be derived" >&2
    exit 64
  fi
fi

if [[ -z "${state_root}" ]]; then
  if [[ -n "${XDG_STATE_HOME:-}" ]]; then
    state_root="${XDG_STATE_HOME%/}/neovex/machine"
  elif [[ -n "${home_dir}" ]]; then
    state_root="${home_dir%/}/.local/state/neovex/machine"
  else
    echo "set --state-root or provide --home/HOME so the state root can be derived" >&2
    exit 64
  fi
fi

if [[ -z "${output_dir}" ]]; then
  output_dir="$(mktemp -d "${TMPDIR:-/tmp}/neovex-machine-diag.XXXXXX")"
else
  mkdir -p "${output_dir}"
fi

output_dir="$(cd "${output_dir}" && pwd)"
summary_file="${output_dir}/summary.txt"
: > "${summary_file}"

config_dir="${config_root%/}/${machine_name}"
state_dir="${state_root%/}/${machine_name}"
runtime_dir="${runtime_root%/}"
config_path="${config_dir}/config.json"
state_path="${state_dir}/status.json"
api_socket="${runtime_dir}/${machine_name}-api.sock"
ready_socket="${runtime_dir}/${machine_name}.sock"
ignition_socket="${runtime_dir}/${machine_name}-ignition.sock"
gvproxy_socket="${runtime_dir}/${machine_name}-gvproxy.sock"
krunkit_socket="${runtime_dir}/${machine_name}-krunkit.sock"
machine_log="${runtime_dir}/${machine_name}.log"
gvproxy_log="${runtime_dir}/${machine_name}-gvproxy.log"
krunkit_log="${runtime_dir}/${machine_name}-krunkit.log"
gvproxy_pid_file="${runtime_dir}/${machine_name}-gvproxy.pid"
krunkit_pid_file="${runtime_dir}/${machine_name}-krunkit.pid"

print_line "output.dir" "${output_dir}"
print_line "machine.name" "${machine_name}"
print_line "home.dir" "${home_dir:-<unset>}"
print_line "root.config" "${config_root}"
print_line "root.state" "${state_root}"
print_line "root.runtime" "${runtime_root}"
print_line "path.config" "${config_path}"
print_line "path.state" "${state_path}"
print_line "path.runtime_root" "${runtime_dir}"
print_line "path.api_socket" "${api_socket}"
print_line "path.ready_socket" "${ready_socket}"
print_line "path.ignition_socket" "${ignition_socket}"
print_line "path.gvproxy_socket" "${gvproxy_socket}"
print_line "path.krunkit_socket" "${krunkit_socket}"

copy_if_present "artifact.machine_config" "${config_path}" "${output_dir}/machine-config.json"
copy_if_present "artifact.machine_state" "${state_path}" "${output_dir}/machine-state.json"
list_dir_if_present "artifact.runtime_dir" "${runtime_dir}" "${output_dir}/runtime-dir-listing.txt"
copy_if_present "artifact.gvproxy_pid" "${gvproxy_pid_file}" "${output_dir}/gvproxy.pid"
copy_if_present "artifact.krunkit_pid" "${krunkit_pid_file}" "${output_dir}/krunkit.pid"
tail_file_if_present "artifact.machine_log" "${machine_log}" "${output_dir}/machine-log-tail.txt" "${log_lines}"
tail_file_if_present "artifact.gvproxy_log" "${gvproxy_log}" "${output_dir}/gvproxy-log-tail.txt" "${log_lines}"
tail_file_if_present "artifact.krunkit_log" "${krunkit_log}" "${output_dir}/krunkit-log-tail.txt" "${log_lines}"

{
  printf '%s %s\n' "$(basename "${api_socket}")" "$(if [[ -e "${api_socket}" ]]; then printf 'present'; else printf 'missing'; fi)"
  printf '%s %s\n' "$(basename "${ready_socket}")" "$(if [[ -e "${ready_socket}" ]]; then printf 'present'; else printf 'missing'; fi)"
  printf '%s %s\n' "$(basename "${ignition_socket}")" "$(if [[ -e "${ignition_socket}" ]]; then printf 'present'; else printf 'missing'; fi)"
  printf '%s %s\n' "$(basename "${gvproxy_socket}")" "$(if [[ -e "${gvproxy_socket}" ]]; then printf 'present'; else printf 'missing'; fi)"
  printf '%s %s\n' "$(basename "${krunkit_socket}")" "$(if [[ -e "${krunkit_socket}" ]]; then printf 'present'; else printf 'missing'; fi)"
} >"${output_dir}/socket-inventory.txt"
print_line "artifact.socket_inventory" "ok path=${output_dir}/socket-inventory.txt"

{
  printf '%s %s\n' "$(basename "${api_socket}")" "$(if [[ -e "${api_socket}" ]]; then printf 'present'; else printf 'missing'; fi)"
  printf '%s %s\n' "$(basename "${ready_socket}")" "$(if [[ -e "${ready_socket}" ]]; then printf 'present'; else printf 'missing'; fi)"
  printf '%s %s\n' "$(basename "${ignition_socket}")" "$(if [[ -e "${ignition_socket}" ]]; then printf 'present'; else printf 'missing'; fi)"
  printf '%s %s\n' "$(basename "${gvproxy_socket}")" "$(if [[ -e "${gvproxy_socket}" ]]; then printf 'present'; else printf 'missing'; fi)"
  printf '%s %s\n' "$(basename "${krunkit_socket}")" "$(if [[ -e "${krunkit_socket}" ]]; then printf 'present'; else printf 'missing'; fi)"
} >"${output_dir}/socket-presence.txt"
print_line "artifact.socket_presence" "ok path=${output_dir}/socket-presence.txt"

capture_processes "${output_dir}/processes-all.txt"
grep -E 'krunkit|gvproxy|neovex' "${output_dir}/processes-all.txt" >"${output_dir}/processes-matching.txt" || true
print_line "capture.processes_filtered" "ok path=${output_dir}/processes-matching.txt"

if [[ -n "${neovex_bin}" ]]; then
  capture_command \
    "capture.neovex_machine_status" \
    "${output_dir}/neovex-machine-status.txt" \
    env \
    "HOME=${home_dir}" \
    "NEOVEX_MACHINE_RUNTIME_ROOT=${runtime_root}" \
    "${neovex_bin}" \
    machine \
    status
fi

print_line "result" "captured"
