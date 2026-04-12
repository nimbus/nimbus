#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
  cat <<'EOF'
usage: recreate-podman-machine.sh --machine <name> [options]

Recreate a Podman machine from scratch using the short-runtime-dir discipline
that cleared the Darwin unix-socket path overflow on the current macOS host.
This helper optionally captures pre-recreate diagnostics, force-removes the
existing machine if present, reinitializes it with a known mount/CPU/memory
shape, starts it, and captures a post-start readiness bundle.

options:
  --machine <name>             Podman machine name to recreate (required)
  --connection <name>          Podman connection name for readiness checks
                               (default: same as --machine)
  --provider <name>            Provider to force for Podman commands
                               (default: libkrun)
  --tmp-root <path>            Podman runtime tmp root. Must end in `/podman`
                               because Podman derives it from `TMPDIR`.
                               (default: ${TMPDIR:-/tmp}/podman)
  --output-dir <path>          Output directory for all recreate artifacts
  --cpus <count>               CPU count for `podman machine init`
                               (default: 2)
  --memory <mib>               Memory MiB for `podman machine init`
                               (default: 2048)
  --disk-size <gib>            Disk size GiB for `podman machine init`
                               (default: 20)
  --volume <host:guest>        Volume to pass through `podman machine init -v`
                               Repeat for multiple mounts. Default:
                               `/Users:/Users`
  --skip-pre-diagnostics       Skip pre-recreate diagnostics capture
  --podman <path>              Override Podman binary path
  --ps <path>                  Override ps command path
  --system-profiler <path>     Override system_profiler path
  --log-lines <count>          Number of machine-log lines for diagnostics
                               (default: 120)
  --ssh-command <command>      Guest command for readiness validation
                               (default: uname -a)
  -h, --help                   Show this help

examples:
  TMPDIR=/tmp bash scripts/recreate-podman-machine.sh \
    --machine neovex-libkrun-users-only \
    --provider libkrun \
    --tmp-root /tmp/podman \
    --output-dir /tmp/neovex-libkrun-users-only-recreate
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

write_command_file() {
  local output_path="$1"
  shift

  local -a rendered=()
  local arg=""

  for arg in "$@"; do
    rendered+=( "$(printf '%q' "${arg}")" )
  done

  printf '%s\n' "${rendered[*]}" > "${output_path}"
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

  return "${status}"
}

capture_podman() {
  local label="$1"
  local output_path="$2"
  shift 2

  capture_command \
    "${label}" \
    "${output_path}" \
    env \
    "TMPDIR=${tmp_parent}" \
    "CONTAINERS_MACHINE_PROVIDER=${provider}" \
    "${podman_bin}" \
    "$@"
}

machine_name=""
connection_name=""
provider="libkrun"
tmp_root="${TMPDIR:-/tmp}/podman"
output_dir=""
cpus=2
memory=2048
disk_size=20
skip_pre_diagnostics=0
podman_bin="podman"
ps_bin="ps"
system_profiler_bin="system_profiler"
log_lines=120
ssh_command="uname -a"
volumes=()

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
    --cpus)
      cpus="${2:?missing cpu count}"
      shift 2
      ;;
    --memory)
      memory="${2:?missing memory amount}"
      shift 2
      ;;
    --disk-size)
      disk_size="${2:?missing disk size}"
      shift 2
      ;;
    --volume)
      volumes+=( "${2:?missing volume}" )
      shift 2
      ;;
    --skip-pre-diagnostics)
      skip_pre_diagnostics=1
      shift
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
if [[ "$(basename "${tmp_root}")" != "podman" ]]; then
  echo "--tmp-root must end in /podman because Podman derives the runtime dir from TMPDIR" >&2
  exit 64
fi
tmp_parent="$(dirname "${tmp_root}")"
mkdir -p "${tmp_parent}"

if [[ ${#volumes[@]} -eq 0 ]]; then
  volumes=( "/Users:/Users" )
fi

if [[ -z "${output_dir}" ]]; then
  output_dir="$(mktemp -d "${TMPDIR:-/tmp}/${machine_name}-podman-machine-recreate.XXXXXX")"
else
  mkdir -p "${output_dir}"
fi

output_dir="$(cd "${output_dir}" && pwd)"
summary_file="${output_dir}/summary.txt"
: > "${summary_file}"

print_line "output.dir" "${output_dir}"
print_line "machine.name" "${machine_name}"
print_line "podman.connection" "${connection_name}"
print_line "machine.provider" "${provider}"
print_line "artifacts.tmp_parent" "${tmp_parent}"
print_line "artifacts.tmp_root" "${tmp_root}"
print_line "machine.cpus" "${cpus}"
print_line "machine.memory_mib" "${memory}"
print_line "machine.disk_size_gib" "${disk_size}"
print_line "machine.volumes" "$(IFS=,; printf '%s' "${volumes[*]}")"
print_line "pre_diagnostics.enabled" "$([[ "${skip_pre_diagnostics}" -eq 0 ]] && printf yes || printf no)"
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
  "${script_dir}/check-podman-machine-socket-paths.sh" \
  --machine "${machine_name}" \
  --tmp-root "${tmp_root}"

capture_podman \
  "capture.machine_list_before" \
  "${output_dir}/podman-machine-list-before.txt" \
  machine list --all-providers --format json || true

machine_exists="no"
if capture_podman \
  "capture.machine_inspect_before" \
  "${output_dir}/podman-machine-inspect-before.txt" \
  machine inspect "${machine_name}"; then
  machine_exists="yes"
fi
print_line "machine.preexisting" "${machine_exists}"

if [[ "${machine_exists}" == "yes" && "${skip_pre_diagnostics}" -eq 0 ]]; then
  capture_command \
    "capture.pre_diagnostics" \
    "${output_dir}/pre-diagnostics-run.txt" \
    bash \
    "${script_dir}/collect-podman-machine-diagnostics.sh" \
    --machine "${machine_name}" \
    --provider "${provider}" \
    --tmp-root "${tmp_root}" \
    --output-dir "${output_dir}/pre-diagnostics" \
    --podman "${podman_bin}" \
    --ps "${ps_bin}" \
    --system-profiler "${system_profiler_bin}" \
    --log-lines "${log_lines}" || true
else
  print_line "capture.pre_diagnostics" "skipped machine_exists=${machine_exists} enabled=$([[ "${skip_pre_diagnostics}" -eq 0 ]] && printf yes || printf no)"
fi

remove_status=0
if [[ "${machine_exists}" == "yes" ]]; then
  write_command_file \
    "${output_dir}/podman-machine-rm-command.txt" \
    env \
    "TMPDIR=${tmp_parent}" \
    "CONTAINERS_MACHINE_PROVIDER=${provider}" \
    "${podman_bin}" \
    machine rm -f "${machine_name}"

  set +e
  capture_podman \
    "recreate.remove_existing" \
    "${output_dir}/podman-machine-rm.txt" \
    machine rm -f "${machine_name}"
  remove_status=$?
  set -e
else
  print_line "recreate.remove_existing" "skipped machine_missing"
fi

init_status=0
if [[ "${remove_status}" -eq 0 ]]; then
  init_args=( machine init --cpus "${cpus}" --memory "${memory}" --disk-size "${disk_size}" )
  for volume in "${volumes[@]}"; do
    init_args+=( -v "${volume}" )
  done
  init_args+=( "${machine_name}" )

  write_command_file \
    "${output_dir}/podman-machine-init-command.txt" \
    env \
    "TMPDIR=${tmp_parent}" \
    "CONTAINERS_MACHINE_PROVIDER=${provider}" \
    "${podman_bin}" \
    "${init_args[@]}"

  set +e
  capture_podman \
    "recreate.init" \
    "${output_dir}/podman-machine-init.txt" \
    "${init_args[@]}"
  init_status=$?
  set -e
else
  print_line "recreate.init" "skipped remove_failed status=${remove_status}"
fi

start_status=0
if [[ "${remove_status}" -eq 0 && "${init_status}" -eq 0 ]]; then
  write_command_file \
    "${output_dir}/podman-machine-start-command.txt" \
    env \
    "TMPDIR=${tmp_parent}" \
    "CONTAINERS_MACHINE_PROVIDER=${provider}" \
    "${podman_bin}" \
    machine start "${machine_name}"

  set +e
  capture_podman \
    "recreate.start" \
    "${output_dir}/podman-machine-start.txt" \
    machine start "${machine_name}"
  start_status=$?
  set -e
else
  print_line "recreate.start" "skipped init_failed status=${init_status}"
fi

capture_podman \
  "capture.machine_list_after" \
  "${output_dir}/podman-machine-list-after.txt" \
  machine list --all-providers --format json || true

readiness_status=0
set +e
capture_command \
  "capture.readiness" \
  "${output_dir}/readiness-run.txt" \
  bash \
  "${script_dir}/validate-podman-machine-readiness.sh" \
  --machine "${machine_name}" \
  --connection "${connection_name}" \
  --provider "${provider}" \
  --tmp-root "${tmp_root}" \
  --output-dir "${output_dir}/readiness" \
  --podman "${podman_bin}" \
  --ps "${ps_bin}" \
  --system-profiler "${system_profiler_bin}" \
  --log-lines "${log_lines}" \
  --ssh-command "${ssh_command}"
readiness_status=$?
set -e

readiness_result="unknown"
readiness_summary="${output_dir}/readiness/summary.txt"
if [[ -f "${readiness_summary}" ]]; then
  readiness_result="$(awk '/^result[[:space:]]+/ { $1=""; sub(/^ +/, ""); print; exit }' "${readiness_summary}")"
fi
print_line "readiness.result" "${readiness_result}"

final_result="not_ready"
exit_code=1

if [[ "${remove_status}" -eq 0 && "${init_status}" -eq 0 && "${start_status}" -eq 0 && "${readiness_status}" -eq 0 && "${readiness_result}" == ready* ]]; then
  final_result="ready"
  exit_code=0
fi

print_line "result" "${final_result} remove_status=${remove_status} init_status=${init_status} start_status=${start_status} readiness_status=${readiness_status}"

exit "${exit_code}"
