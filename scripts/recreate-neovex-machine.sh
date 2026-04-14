#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"

usage() {
  cat <<'EOF'
usage: recreate-neovex-machine.sh --image <path> [options]

Recreate a Neovex macOS machine using the shipped `neovex machine ...` surface.
This is the Neovex-owned MAC3 repair lane for stale config/runtime state.

options:
  --machine <name>             Machine name (default: default)
  --home <path>                HOME to use for XDG-style machine roots
  --runtime-root <path>        Runtime root (default: /tmp/neovex)
  --output-dir <path>          Output directory for recreate artifacts
  --neovex <path>              Neovex binary path
                               (default: <repo>/target/debug/neovex)
  --image <path>               Bootable local guest disk image (required)
  --ssh-identity <path>        SSH identity path for guest debugging
  --ignition-file <path>       Ignition file to serve over first-boot vsock
  --efi-store <path>           EFI variable-store path
  --cpus <count>               Guest CPU count (default: 2)
  --memory-mib <mib>           Guest memory MiB (default: 2048)
  --disk-gib <gib>             Guest disk size GiB (default: 20)
  --volume <host:guest>        virtiofs mount (repeatable; default: /Users:/Users)
  --skip-pre-diagnostics       Skip pre-recreate diagnostics capture
  --log-lines <count>          Log lines to include in diagnostics bundles
  -h, --help                   Show this help

examples:
  bash scripts/recreate-neovex-machine.sh \
    --home /tmp/neovex-home \
    --runtime-root /tmp/neovex \
    --image /absolute/path/to/neovex-machine.raw \
    --ssh-identity /absolute/path/to/machine-key \
    --ignition-file /absolute/path/to/neovex-machine.ign
EOF
}

print_line() {
  local label="$1"
  local value="$2"
  printf '%-34s %s\n' "${label}" "${value}" | tee -a "${summary_file}"
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

capture_command_allow_failure() {
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

machine_name="default"
home_dir="${HOME:-}"
runtime_root="${NEOVEX_MACHINE_RUNTIME_ROOT:-/tmp/neovex}"
output_dir=""
neovex_bin="${repo_root}/target/debug/neovex"
image_path=""
ssh_identity=""
ignition_file=""
efi_store=""
cpus=2
memory_mib=2048
disk_gib=20
skip_pre_diagnostics=0
log_lines=120
volumes=()

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
    --image)
      image_path="${2:?missing image path}"
      shift 2
      ;;
    --ssh-identity)
      ssh_identity="${2:?missing ssh identity path}"
      shift 2
      ;;
    --ignition-file)
      ignition_file="${2:?missing ignition file path}"
      shift 2
      ;;
    --efi-store)
      efi_store="${2:?missing efi store path}"
      shift 2
      ;;
    --cpus)
      cpus="${2:?missing cpu count}"
      shift 2
      ;;
    --memory-mib)
      memory_mib="${2:?missing memory amount}"
      shift 2
      ;;
    --disk-gib)
      disk_gib="${2:?missing disk size}"
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

if [[ -z "${home_dir}" ]]; then
  echo "set --home or HOME so the machine roots can be derived" >&2
  exit 64
fi

if [[ -z "${image_path}" ]]; then
  echo "missing required --image argument" >&2
  usage >&2
  exit 64
fi

if [[ ${#volumes[@]} -eq 0 ]]; then
  volumes=( "/Users:/Users" )
fi

if [[ ! -x "${neovex_bin}" ]]; then
  echo "neovex binary is not executable at ${neovex_bin}; build it first or pass --neovex" >&2
  exit 64
fi

if [[ -z "${output_dir}" ]]; then
  output_dir="$(mktemp -d "${TMPDIR:-/tmp}/neovex-machine-recreate.XXXXXX")"
else
  mkdir -p "${output_dir}"
fi

output_dir="$(cd "${output_dir}" && pwd)"
summary_file="${output_dir}/summary.txt"
: > "${summary_file}"

print_line "output.dir" "${output_dir}"
print_line "machine.name" "${machine_name}"
print_line "home.dir" "${home_dir}"
print_line "runtime.root" "${runtime_root}"
print_line "runtime.layout" "flat root with ${machine_name}-*.sock/${machine_name}-*.log/${machine_name}-*.pid"
print_line "neovex.bin" "${neovex_bin}"
print_line "image.path" "${image_path}"

if [[ "${skip_pre_diagnostics}" -eq 0 ]]; then
  bash "${script_dir}/collect-neovex-machine-diagnostics.sh" \
    --machine "${machine_name}" \
    --home "${home_dir}" \
    --runtime-root "${runtime_root}" \
    --output-dir "${output_dir}/pre-diagnostics" \
    --neovex "${neovex_bin}" \
    --log-lines "${log_lines}" \
    > "${output_dir}/pre-diagnostics-stdout.txt"
  print_line "capture.pre_diagnostics" "ok path=${output_dir}/pre-diagnostics"
fi

stop_cmd=(
  env
  "HOME=${home_dir}"
  "NEOVEX_MACHINE_RUNTIME_ROOT=${runtime_root}"
  "${neovex_bin}"
  machine
  stop
)
write_command_file "${output_dir}/neovex-machine-stop-command.txt" "${stop_cmd[@]}"
capture_command_allow_failure \
  "recreate.stop_existing" \
  "${output_dir}/neovex-machine-stop.txt" \
  "${stop_cmd[@]}" || true

rm_cmd=(
  env
  "HOME=${home_dir}"
  "NEOVEX_MACHINE_RUNTIME_ROOT=${runtime_root}"
  "${neovex_bin}"
  machine
  rm
)
write_command_file "${output_dir}/neovex-machine-rm-command.txt" "${rm_cmd[@]}"
capture_command_allow_failure \
  "recreate.remove_existing" \
  "${output_dir}/neovex-machine-rm.txt" \
  "${rm_cmd[@]}" || true

init_cmd=(
  env
  "HOME=${home_dir}"
  "NEOVEX_MACHINE_RUNTIME_ROOT=${runtime_root}"
  "${neovex_bin}"
  machine
  init
  --cpus "${cpus}"
  --memory-mib "${memory_mib}"
  --disk-gib "${disk_gib}"
  --image "${image_path}"
)

if [[ -n "${ssh_identity}" ]]; then
  init_cmd+=( --ssh-identity "${ssh_identity}" )
fi
if [[ -n "${ignition_file}" ]]; then
  init_cmd+=( --ignition-file "${ignition_file}" )
fi
if [[ -n "${efi_store}" ]]; then
  init_cmd+=( --efi-store "${efi_store}" )
fi

for volume in "${volumes[@]}"; do
  init_cmd+=( --volume "${volume}" )
done

write_command_file "${output_dir}/neovex-machine-init-command.txt" "${init_cmd[@]}"
set +e
capture_command_allow_failure \
  "recreate.init" \
  "${output_dir}/neovex-machine-init.txt" \
  "${init_cmd[@]}"
init_status=$?
set -e

start_status=0
if [[ "${init_status}" -eq 0 ]]; then
  start_cmd=(
    env
    "HOME=${home_dir}"
    "NEOVEX_MACHINE_RUNTIME_ROOT=${runtime_root}"
    "${neovex_bin}"
    machine
    start
  )
  write_command_file "${output_dir}/neovex-machine-start-command.txt" "${start_cmd[@]}"
  set +e
  capture_command_allow_failure \
    "recreate.start" \
    "${output_dir}/neovex-machine-start.txt" \
    "${start_cmd[@]}"
  start_status=$?
  set -e
fi

status_cmd=(
  env
  "HOME=${home_dir}"
  "NEOVEX_MACHINE_RUNTIME_ROOT=${runtime_root}"
  "${neovex_bin}"
  machine
  status
)
write_command_file "${output_dir}/neovex-machine-status-command.txt" "${status_cmd[@]}"
capture_command_allow_failure \
  "capture.final_status" \
  "${output_dir}/neovex-machine-status.txt" \
  "${status_cmd[@]}" || true

bash "${script_dir}/collect-neovex-machine-diagnostics.sh" \
  --machine "${machine_name}" \
  --home "${home_dir}" \
  --runtime-root "${runtime_root}" \
  --output-dir "${output_dir}/post-diagnostics" \
  --neovex "${neovex_bin}" \
  --log-lines "${log_lines}" \
  > "${output_dir}/post-diagnostics-stdout.txt"
print_line "capture.post_diagnostics" "ok path=${output_dir}/post-diagnostics"

if [[ "${init_status}" -ne 0 || "${start_status}" -ne 0 ]]; then
  print_line "result" "failed"
  exit 1
fi

print_line "result" "ready"
