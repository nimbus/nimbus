#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: collect-podman-machine-diagnostics.sh --machine <name> [options]

Collect best-effort Podman machine diagnostics into a deterministic output
directory. This helper is intended for the macOS Podman machine research lane
in the VMM infrastructure plan, especially when a libkrun-backed guest fails
to become ready.

options:
  --machine <name>             Podman machine name to inspect (required)
  --provider <name>            Provider to force for Podman commands
  --output-dir <path>          Output directory for captured artifacts
  --config-root <path>         Override Podman machine config root
  --data-root <path>           Override Podman machine data root
  --tmp-root <path>            Override Podman runtime tmp root
  --podman <path>              Override Podman binary path
  --ps <path>                  Override ps command path
  --system-profiler <path>     Override system_profiler path
  --log-lines <count>          Number of log lines to copy from the machine log
  -h, --help                   Show this help

examples:
  bash scripts/collect-podman-machine-diagnostics.sh \
    --machine neovex-libkrun-validation \
    --provider libkrun \
    --output-dir /tmp/neovex-libkrun-diagnostics
EOF
}

print_line() {
  local label="$1"
  local value="$2"
  printf '%-34s %s\n' "${label}" "${value}" | tee -a "${summary_file}"
}

compact_value() {
  printf '%s' "$1" | tr '\n' ' ' | sed -e 's/[[:space:]]\+/ /g' -e 's/^ //' -e 's/ $//'
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
    return 0
  fi

  print_line "${label}" "failed status=${status} path=${output_path}"
  return 0
}

capture_podman() {
  local label="$1"
  local output_path="$2"
  shift 2

  if [[ "${provider_for_commands}" != "unknown" && "${provider_for_commands}" != "ambiguous" ]]; then
    capture_command "${label}" "${output_path}" env "CONTAINERS_MACHINE_PROVIDER=${provider_for_commands}" "${podman_bin}" "$@"
    return 0
  fi

  capture_command "${label}" "${output_path}" "${podman_bin}" "$@"
}

machine_name=""
provider=""
provider_source="auto"
podman_bin="podman"
ps_bin="ps"
system_profiler_bin="system_profiler"
config_root="${HOME}/.config/containers/podman/machine"
data_root="${HOME}/.local/share/containers/podman/machine"
tmp_root="${TMPDIR:-/tmp}/podman"
output_dir=""
log_lines=120

while [[ $# -gt 0 ]]; do
  case "$1" in
    --machine)
      machine_name="${2:?missing machine name}"
      shift 2
      ;;
    --provider)
      provider="${2:?missing provider}"
      provider_source="arg"
      shift 2
      ;;
    --output-dir)
      output_dir="${2:?missing output directory}"
      shift 2
      ;;
    --config-root)
      config_root="${2:?missing config root}"
      shift 2
      ;;
    --data-root)
      data_root="${2:?missing data root}"
      shift 2
      ;;
    --tmp-root)
      tmp_root="${2:?missing tmp root}"
      shift 2
      ;;
    --podman)
      podman_bin="${2:?missing podman path}"
      shift 2
      ;;
    --ps)
      ps_bin="${2:?missing ps path}"
      shift 2
      ;;
    --system-profiler)
      system_profiler_bin="${2:?missing system_profiler path}"
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

if [[ -z "${machine_name}" ]]; then
  echo "missing required --machine argument" >&2
  usage >&2
  exit 64
fi

if [[ -z "${provider}" && -n "${CONTAINERS_MACHINE_PROVIDER:-}" ]]; then
  provider="${CONTAINERS_MACHINE_PROVIDER}"
  provider_source="env"
fi

if [[ -z "${output_dir}" ]]; then
  output_dir="$(mktemp -d "${TMPDIR:-/tmp}/${machine_name}-podman-machine-diagnostics.XXXXXX")"
else
  mkdir -p "${output_dir}"
fi

output_dir="$(cd "${output_dir}" && pwd)"
summary_file="${output_dir}/summary.txt"
: > "${summary_file}"

resolved_config_path=""
resolved_provider_path=""

if [[ -z "${provider}" ]]; then
  shopt -s nullglob
  config_matches=( "${config_root}"/*/"${machine_name}.json" )
  shopt -u nullglob

  if [[ ${#config_matches[@]} -eq 1 ]]; then
    resolved_config_path="${config_matches[0]}"
    provider="$(basename "$(dirname "${resolved_config_path}")")"
    provider_source="config"
  elif [[ ${#config_matches[@]} -gt 1 ]]; then
    provider="ambiguous"
    provider_source="config"
  else
    provider="unknown"
    provider_source="unresolved"
  fi
fi

provider_for_commands="${provider}"

if [[ -z "${resolved_config_path}" && "${provider}" != "unknown" && "${provider}" != "ambiguous" ]]; then
  candidate_config_path="${config_root}/${provider}/${machine_name}.json"
  if [[ -f "${candidate_config_path}" ]]; then
    resolved_config_path="${candidate_config_path}"
  fi
fi

if [[ -n "${resolved_config_path}" ]]; then
  resolved_provider_path="$(dirname "${resolved_config_path}")"
elif [[ "${provider}" != "unknown" && "${provider}" != "ambiguous" ]]; then
  resolved_provider_path="${config_root}/${provider}"
fi

resolved_disk_path=""
if [[ "${provider}" != "unknown" && "${provider}" != "ambiguous" ]]; then
  shopt -s nullglob
  disk_matches=( "${data_root}/${provider}/${machine_name}"-* )
  shopt -u nullglob
  if [[ ${#disk_matches[@]} -gt 0 ]]; then
    resolved_disk_path="${disk_matches[0]}"
  fi
fi

resolved_log_path="${tmp_root}/${machine_name}.log"
resolved_api_socket="${tmp_root}/${machine_name}-api.sock"
resolved_ready_socket="${tmp_root}/${machine_name}.sock"
resolved_gvproxy_socket="${tmp_root}/${machine_name}-gvproxy.sock"

print_line "output.dir" "${output_dir}"
print_line "machine.name" "${machine_name}"
print_line "machine.provider" "${provider}"
print_line "machine.provider_source" "${provider_source}"
print_line "host.os" "$(uname -s)"
print_line "host.arch" "$(uname -m)"
print_line "host.kernel" "$(uname -r)"
print_line "artifacts.config_root" "${config_root}"
print_line "artifacts.data_root" "${data_root}"
print_line "artifacts.tmp_root" "${tmp_root}"

if command -v sw_vers >/dev/null 2>&1; then
  print_line "host.sw_vers" "$(compact_value "$(sw_vers 2>/dev/null || true)")"
fi

if command -v "${podman_bin}" >/dev/null 2>&1; then
  print_line "tool.podman" "present path=$(command -v "${podman_bin}")"
  capture_podman "capture.podman_version" "${output_dir}/podman-version.txt" --version
  capture_podman "capture.podman_info_debug" "${output_dir}/podman-info-debug.txt" info --debug
  capture_podman "capture.machine_list" "${output_dir}/podman-machine-list.json" machine list --all-providers --format json
  capture_podman "capture.machine_inspect" "${output_dir}/podman-machine-inspect.txt" machine inspect "${machine_name}"
else
  print_line "tool.podman" "missing path=${podman_bin}"
fi

if [[ -n "${resolved_config_path}" && -f "${resolved_config_path}" ]]; then
  cp "${resolved_config_path}" "${output_dir}/machine-config.json"
  print_line "artifact.machine_config" "present source=${resolved_config_path} copy=${output_dir}/machine-config.json"
else
  print_line "artifact.machine_config" "missing"
fi

if [[ -n "${resolved_disk_path}" && -e "${resolved_disk_path}" ]]; then
  print_line "artifact.machine_disk" "present path=${resolved_disk_path}"
else
  print_line "artifact.machine_disk" "missing"
fi

if [[ -d "${tmp_root}" ]]; then
  capture_command "capture.tmp_root_listing" "${output_dir}/tmp-root-listing.txt" ls -l "${tmp_root}"
else
  print_line "capture.tmp_root_listing" "missing tmp_root=${tmp_root}"
fi

if [[ -e "${resolved_log_path}" ]]; then
  tail -n "${log_lines}" "${resolved_log_path}" > "${output_dir}/machine-log-tail.txt"
  print_line "artifact.machine_log" "present path=${resolved_log_path} tail=${output_dir}/machine-log-tail.txt"
else
  print_line "artifact.machine_log" "missing path=${resolved_log_path}"
fi

for socket_path in \
  "${resolved_api_socket}" \
  "${resolved_ready_socket}" \
  "${resolved_gvproxy_socket}"
do
  socket_label="$(basename "${socket_path}")"
  if [[ -e "${socket_path}" ]]; then
    print_line "artifact.${socket_label}" "present path=${socket_path}"
  else
    print_line "artifact.${socket_label}" "missing path=${socket_path}"
  fi
done

if command -v "${ps_bin}" >/dev/null 2>&1; then
  ps_status=0
  set +e
  "${ps_bin}" -axww -o pid=,ppid=,stat=,command= > "${output_dir}/processes-all.txt" 2>&1
  ps_status=$?
  set -e

  if [[ "${ps_status}" -eq 0 ]]; then
    awk -v machine_name="${machine_name}" '
      index($0, machine_name) || index($0, "krunkit") || index($0, "gvproxy")
    ' "${output_dir}/processes-all.txt" > "${output_dir}/processes-matching.txt"
    print_line "capture.processes" "ok all=${output_dir}/processes-all.txt filtered=${output_dir}/processes-matching.txt"
  else
    print_line "capture.processes" "failed status=${ps_status} path=${output_dir}/processes-all.txt"
  fi
else
  print_line "capture.processes" "missing path=${ps_bin}"
fi

if [[ "$(uname -s)" == "Darwin" && -x "$(command -v "${system_profiler_bin}" 2>/dev/null || true)" ]]; then
  capture_command "capture.system_profiler_hardware" "${output_dir}/system-profiler-hardware.txt" "${system_profiler_bin}" SPHardwareDataType
  capture_command "capture.system_profiler_software" "${output_dir}/system-profiler-software.txt" "${system_profiler_bin}" SPSoftwareDataType
fi

print_line "result" "captured"
