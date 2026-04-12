#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: validate-podman-machine-readiness.sh --machine <name> [options]

Capture a focused readiness bundle for a Podman machine after startup. This is
intended for the macOS libkrun short-runtime-dir validation lane where we want
to prove whether the guest remains reachable via the Podman API and
`podman machine ssh`.

options:
  --machine <name>             Podman machine name to inspect (required)
  --connection <name>          Podman connection name to target
                               (default: same as --machine)
  --provider <name>            Provider to force for Podman commands
  --tmp-root <path>            Podman runtime tmp root
                               (default: ${TMPDIR:-/tmp}/podman)
  --output-dir <path>          Output directory for captured artifacts
  --podman <path>              Override Podman binary path
  --ps <path>                  Override ps command path
  --system-profiler <path>     Override system_profiler path
  --log-lines <count>          Number of machine-log lines to copy
  --ssh-command <command>      Guest command to run via `podman machine ssh`
                               (default: uname -a)
  -h, --help                   Show this help

examples:
  TMPDIR=/tmp bash scripts/validate-podman-machine-readiness.sh \
    --machine neovex-libkrun-users-only \
    --provider libkrun \
    --tmp-root /tmp/podman \
    --output-dir /tmp/neovex-libkrun-users-only-readiness
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
  else
    print_line "${label}" "failed status=${status} path=${output_path}"
  fi

  return 0
}

capture_podman() {
  local label="$1"
  local output_path="$2"
  shift 2

  local -a cmd=( "${podman_bin}" "$@" )

  if [[ -n "${provider}" ]]; then
    capture_command "${label}" "${output_path}" env "CONTAINERS_MACHINE_PROVIDER=${provider}" "${cmd[@]}"
    return 0
  fi

  capture_command "${label}" "${output_path}" "${cmd[@]}"
}

machine_name=""
connection_name=""
provider=""
tmp_root="${TMPDIR:-/tmp}/podman"
output_dir=""
podman_bin="podman"
ps_bin="ps"
system_profiler_bin="system_profiler"
log_lines=120
ssh_command="uname -a"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --machine)
      machine_name="${2:?missing machine name}"
      shift 2
      ;;
    --connection)
      connection_name="${2:?missing connection name}"
      shift 2
      ;;
    --provider)
      provider="${2:?missing provider}"
      shift 2
      ;;
    --tmp-root)
      tmp_root="${2:?missing tmp root}"
      shift 2
      ;;
    --output-dir)
      output_dir="${2:?missing output directory}"
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
    --ssh-command)
      ssh_command="${2:?missing ssh command}"
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

if [[ -z "${connection_name}" ]]; then
  connection_name="${machine_name}"
fi

tmp_root="${tmp_root%/}"

if [[ -z "${output_dir}" ]]; then
  output_dir="$(mktemp -d "${TMPDIR:-/tmp}/${machine_name}-podman-machine-readiness.XXXXXX")"
else
  mkdir -p "${output_dir}"
fi

output_dir="$(cd "${output_dir}" && pwd)"
summary_file="${output_dir}/summary.txt"
: > "${summary_file}"

print_line "output.dir" "${output_dir}"
print_line "machine.name" "${machine_name}"
print_line "podman.connection" "${connection_name}"
print_line "machine.provider" "${provider:-auto}"
print_line "artifacts.tmp_root" "${tmp_root}"
print_line "host.os" "$(uname -s)"
print_line "host.arch" "$(uname -m)"
print_line "host.kernel" "$(uname -r)"

if command -v sw_vers >/dev/null 2>&1; then
  print_line "host.sw_vers" "$(compact_value "$(sw_vers 2>/dev/null || true)")"
fi

capture_command \
  "capture.socket_budget" \
  "${output_dir}/socket-budget.txt" \
  bash \
  "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/check-podman-machine-socket-paths.sh" \
  --machine "${machine_name}" \
  --tmp-root "${tmp_root}"

capture_command \
  "capture.diagnostics" \
  "${output_dir}/diagnostics-run.txt" \
  bash \
  "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/collect-podman-machine-diagnostics.sh" \
  --machine "${machine_name}" \
  --provider "${provider:-libkrun}" \
  --tmp-root "${tmp_root}" \
  --output-dir "${output_dir}/diagnostics" \
  --podman "${podman_bin}" \
  --ps "${ps_bin}" \
  --system-profiler "${system_profiler_bin}" \
  --log-lines "${log_lines}"

if command -v "${podman_bin}" >/dev/null 2>&1; then
  capture_podman "capture.connection_list" "${output_dir}/podman-system-connection-list.txt" system connection list
  capture_podman "capture.machine_inspect" "${output_dir}/podman-machine-inspect.txt" machine inspect "${machine_name}"
  capture_podman "capture.info_via_connection" "${output_dir}/podman-info-connection.txt" --connection "${connection_name}" info --debug

  ssh_command_file="${output_dir}/machine-ssh-command.txt"
  printf '%s\n' "${ssh_command}" > "${ssh_command_file}"

  capture_podman \
    "capture.machine_ssh" \
    "${output_dir}/podman-machine-ssh.txt" \
    machine ssh "${machine_name}" "${ssh_command}"
else
  print_line "tool.podman" "missing path=${podman_bin}"
fi

if [[ -d "${tmp_root}" ]]; then
  capture_command "capture.tmp_root_listing" "${output_dir}/tmp-root-listing.txt" ls -la "${tmp_root}"
fi

if command -v "${ps_bin}" >/dev/null 2>&1; then
  capture_command "capture.processes" "${output_dir}/processes.txt" "${ps_bin}" -axww -o pid=,ppid=,stat=,command=
fi

info_status="unknown"
ssh_status="unknown"

if [[ -f "${output_dir}/summary.txt" ]]; then
  if grep -F "capture.info_via_connection" "${summary_file}" | grep -F "ok path=" >/dev/null 2>&1; then
    info_status="ok"
  elif grep -F "capture.info_via_connection" "${summary_file}" >/dev/null 2>&1; then
    info_status="failed"
  fi

  if grep -F "capture.machine_ssh" "${summary_file}" | grep -F "ok path=" >/dev/null 2>&1; then
    ssh_status="ok"
  elif grep -F "capture.machine_ssh" "${summary_file}" >/dev/null 2>&1; then
    ssh_status="failed"
  fi
fi

if [[ "${info_status}" == "ok" && "${ssh_status}" == "ok" ]]; then
  print_line "result" "ready info=${info_status} ssh=${ssh_status}"
else
  print_line "result" "not_ready info=${info_status} ssh=${ssh_status}"
fi
