#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"

usage() {
  cat <<'EOF'
usage: recreate-nimbus-machine.sh [options]

Recreate a Nimbus macOS machine using the shipped `nimbus machine ...` surface.
This is the Nimbus-owned macOS repair lane for stale config/runtime state.
By default it follows the current Nimbus bootc machine-config contract recorded
by `nimbus machine init`; `--image` is only for explicit diagnostic overrides.

options:
  --machine <name>             Machine name (default: default)
  --home <path>                HOME to use for XDG-style machine roots
  --runtime-root <path>        Runtime root (default: /tmp/nimbus)
  --output-dir <path>          Output directory for recreate artifacts
  --nimbus <path>              Nimbus binary path
                               (default: <repo>/target/debug/nimbus)
  --image <source>             Explicit machine image source override passed to
                               `nimbus machine init` for diagnostics only
  --bootc-native               Treat an explicit image override as a
                               bootc-native Nimbus machine image
  --machine-os-repo <path>     machine-os checkout used to build a local bootc
                               image when NIMBUS_MACHINE_GUEST_BINARY is set
                               (default: $NIMBUS_MACHINE_OS_REPO or ../machine-os)
  --identity <path>            SSH identity path for guest debugging
  --ignition-path <path>       Legacy Ignition file for explicit Podman image
                               diagnostic overrides only
  --firmware <path>            EFI variable-store path
  --cpus <count>               Guest CPU count (default: 2)
  --memory <mib>               Guest memory MiB (default: 2048)
  --disk-size <gib>            Guest disk size GiB (default: 20)
  --volume <host:guest>        virtiofs mount (repeatable; default: /Users:/Users)
  --skip-pre-diagnostics       Skip pre-recreate diagnostics capture
  --log-lines <count>          Log lines to include in diagnostics bundles
  -h, --help                   Show this help

examples:
  bash scripts/recreate-nimbus-machine.sh \
    --home /tmp/nimbus-home \
    --runtime-root /tmp/nimbus \
    --identity /absolute/path/to/machine-key
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

resolve_machine_os_repo() {
  if [[ -n "${machine_os_repo}" ]]; then
    printf '%s\n' "${machine_os_repo}"
    return 0
  fi
  if [[ -n "${NIMBUS_MACHINE_OS_REPO:-}" ]]; then
    printf '%s\n' "${NIMBUS_MACHINE_OS_REPO}"
    return 0
  fi
  local sibling_repo
  sibling_repo="$(cd "${repo_root}/.." && pwd)/machine-os"
  if [[ -f "${sibling_repo}/scripts/build.sh" ]]; then
    printf '%s\n' "${sibling_repo}"
    return 0
  fi
  echo "set --machine-os-repo or NIMBUS_MACHINE_OS_REPO to a machine-os checkout before using NIMBUS_MACHINE_GUEST_BINARY with the bootc-native default" >&2
  return 64
}

source_revision_for_local_bootc() {
  if [[ -n "${NIMBUS_MACHINE_OS_LOCAL_SOURCE_REVISION:-}" ]]; then
    printf '%s\n' "${NIMBUS_MACHINE_OS_LOCAL_SOURCE_REVISION}"
    return 0
  fi
  git -C "${repo_root}" rev-parse HEAD 2>/dev/null || printf 'unknown\n'
}

machine_name="default"
home_dir="${HOME:-}"
runtime_root="${NIMBUS_MACHINE_RUNTIME_ROOT:-/tmp/nimbus}"
output_dir=""
nimbus_bin="${repo_root}/target/debug/nimbus"
image_path=""
bootc_native=0
machine_os_repo=""
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
    --nimbus)
      nimbus_bin="${2:?missing nimbus path}"
      shift 2
      ;;
    --image)
      image_path="${2:?missing image path}"
      shift 2
      ;;
    --bootc-native)
      bootc_native=1
      shift
      ;;
    --machine-os-repo)
      machine_os_repo="${2:?missing machine-os repo path}"
      shift 2
      ;;
    --identity)
      ssh_identity="${2:?missing ssh identity path}"
      shift 2
      ;;
    --ignition-path)
      ignition_file="${2:?missing ignition file path}"
      shift 2
      ;;
    --firmware)
      efi_store="${2:?missing efi store path}"
      shift 2
      ;;
    --cpus)
      cpus="${2:?missing cpu count}"
      shift 2
      ;;
    --memory)
      memory_mib="${2:?missing memory amount}"
      shift 2
      ;;
    --disk-size)
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

if [[ ${#volumes[@]} -eq 0 ]]; then
  volumes=( "/Users:/Users" )
fi

if [[ ! -x "${nimbus_bin}" ]]; then
  echo "nimbus binary is not executable at ${nimbus_bin}; build it first or pass --nimbus" >&2
  exit 64
fi

if [[ -z "${output_dir}" ]]; then
  output_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-machine-recreate.XXXXXX")"
else
  mkdir -p "${output_dir}"
fi

output_dir="$(cd "${output_dir}" && pwd)"
summary_file="${output_dir}/summary.txt"
: > "${summary_file}"

local_bootc_image_built=0
if [[ -z "${image_path}" && -n "${NIMBUS_MACHINE_GUEST_BINARY:-}" ]]; then
  if [[ -n "${ignition_file}" ]]; then
    echo "--ignition-path is only supported with an explicit Podman machine-os --image override; local bootc builds use machine-config provisioning" >&2
    exit 64
  fi
  if [[ ! -x "${NIMBUS_MACHINE_GUEST_BINARY}" ]]; then
    echo "NIMBUS_MACHINE_GUEST_BINARY is not executable at ${NIMBUS_MACHINE_GUEST_BINARY}" >&2
    exit 64
  fi
  resolved_machine_os_repo="$(resolve_machine_os_repo)"
  if [[ ! -f "${resolved_machine_os_repo}/scripts/build.sh" ]]; then
    echo "machine-os build script not found at ${resolved_machine_os_repo}/scripts/build.sh" >&2
    exit 64
  fi
  local_bootc_output_dir="${output_dir}/local-bootc-image"
  local_bootc_version="${NIMBUS_MACHINE_OS_LOCAL_VERSION:-dev-local}"
  local_bootc_revision="$(source_revision_for_local_bootc)"
  local_bootc_build_cmd=(
    bash
    "${resolved_machine_os_repo}/scripts/build.sh"
    --nimbus-binary "${NIMBUS_MACHINE_GUEST_BINARY}"
    --nimbus-version "${local_bootc_version}"
    --source-revision "${local_bootc_revision}"
    --output-dir "${local_bootc_output_dir}"
  )
  write_command_file \
    "${output_dir}/nimbus-machine-local-bootc-build-command.txt" \
    "${local_bootc_build_cmd[@]}"
  capture_command_allow_failure \
    "local_bootc.build" \
    "${output_dir}/nimbus-machine-local-bootc-build.txt" \
    "${local_bootc_build_cmd[@]}"
  image_path="${local_bootc_output_dir}/nimbus-machine-os.raw"
  if [[ ! -f "${image_path}" ]]; then
    echo "local bootc build completed but did not produce ${image_path}" >&2
    exit 70
  fi
  bootc_native=1
  local_bootc_image_built=1
fi

uses_legacy_podman_image=0
if [[ -n "${image_path}" ]]; then
  case "${image_path}" in
    docker://quay.io/podman/machine-os*|quay.io/podman/machine-os*)
      uses_legacy_podman_image=1
      ;;
  esac
fi

if [[ -n "${ignition_file}" && "${uses_legacy_podman_image}" -ne 1 ]]; then
  echo "--ignition-path is only supported by this repair helper with an explicit Podman machine-os --image override; the default Nimbus bootc machine OS uses machine-config provisioning" >&2
  exit 64
fi

print_line "output.dir" "${output_dir}"
print_line "machine.name" "${machine_name}"
print_line "home.dir" "${home_dir}"
print_line "runtime.root" "${runtime_root}"
print_line "runtime.layout" "flat root with ${machine_name}-*.sock/${machine_name}-*.log/${machine_name}-*.pid"
print_line "nimbus.bin" "${nimbus_bin}"
if [[ -n "${image_path}" ]]; then
  print_line "image.source" "${image_path}"
else
  print_line "image.source" "default (Nimbus bootc machine-config contract)"
fi
if [[ "${uses_legacy_podman_image}" -eq 1 ]]; then
  if [[ -n "${NIMBUS_MACHINE_GUEST_BINARY:-}" ]]; then
    print_line "guest.binary.override" "${NIMBUS_MACHINE_GUEST_BINARY}"
  else
    print_line "guest.binary.override" "<release asset>"
  fi
elif [[ "${local_bootc_image_built}" -eq 1 ]]; then
  print_line "guest.binary.override" "${NIMBUS_MACHINE_GUEST_BINARY} (baked into local bootc dev image)"
else
  if [[ -n "${NIMBUS_MACHINE_GUEST_BINARY:-}" ]]; then
    print_line "guest.binary.override" "ignored for bootc-native image; bake it into a local bootc disk"
  else
    print_line "guest.binary.override" "<baked into bootc image>"
  fi
fi
if [[ "${bootc_native}" -eq 1 ]]; then
  print_line "machine.provisioning" "bootc-native"
fi
if [[ -n "${NIMBUS_MACHINE_API_READY_TIMEOUT_SECS:-}" ]]; then
  print_line "machine.api.ready_timeout_secs" "${NIMBUS_MACHINE_API_READY_TIMEOUT_SECS}"
fi
if [[ -n "${NIMBUS_MACHINE_READY_TIMEOUT_SECS:-}" ]]; then
  print_line "machine.ready_timeout_secs" "${NIMBUS_MACHINE_READY_TIMEOUT_SECS}"
fi
if [[ -n "${NIMBUS_MACHINE_SSH_READY_TIMEOUT_SECS:-}" ]]; then
  print_line "machine.ssh.ready_timeout_secs" "${NIMBUS_MACHINE_SSH_READY_TIMEOUT_SECS}"
fi

if [[ "${skip_pre_diagnostics}" -eq 0 ]]; then
  bash "${script_dir}/collect-nimbus-machine-diagnostics.sh" \
    --machine "${machine_name}" \
    --home "${home_dir}" \
    --runtime-root "${runtime_root}" \
    --output-dir "${output_dir}/pre-diagnostics" \
    --nimbus "${nimbus_bin}" \
    --log-lines "${log_lines}" \
    > "${output_dir}/pre-diagnostics-stdout.txt"
  print_line "capture.pre_diagnostics" "ok path=${output_dir}/pre-diagnostics"
fi

stop_cmd=(
  env
  "HOME=${home_dir}"
  "NIMBUS_MACHINE_RUNTIME_ROOT=${runtime_root}"
  "${nimbus_bin}"
  machine
  stop
  "${machine_name}"
)
write_command_file "${output_dir}/nimbus-machine-stop-command.txt" "${stop_cmd[@]}"
capture_command_allow_failure \
  "recreate.stop_existing" \
  "${output_dir}/nimbus-machine-stop.txt" \
  "${stop_cmd[@]}" || true

rm_cmd=(
  env
  "HOME=${home_dir}"
  "NIMBUS_MACHINE_RUNTIME_ROOT=${runtime_root}"
  "${nimbus_bin}"
  machine
  rm
  "${machine_name}"
)
write_command_file "${output_dir}/nimbus-machine-rm-command.txt" "${rm_cmd[@]}"
capture_command_allow_failure \
  "recreate.remove_existing" \
  "${output_dir}/nimbus-machine-rm.txt" \
  "${rm_cmd[@]}" || true

init_cmd=(
  env
  "HOME=${home_dir}"
  "NIMBUS_MACHINE_RUNTIME_ROOT=${runtime_root}"
  "${nimbus_bin}"
  machine
  init
  --cpus "${cpus}"
  --memory "${memory_mib}"
  --disk-size "${disk_gib}"
)

if [[ -n "${image_path}" ]]; then
  init_cmd+=( --image "${image_path}" )
fi
if [[ "${bootc_native}" -eq 1 ]]; then
  init_cmd+=( --bootc-native )
fi

if [[ -n "${ssh_identity}" ]]; then
  init_cmd+=( --identity "${ssh_identity}" )
fi
if [[ -n "${ignition_file}" ]]; then
  init_cmd+=( --ignition-path "${ignition_file}" )
fi
if [[ -n "${efi_store}" ]]; then
  init_cmd+=( --firmware "${efi_store}" )
fi

for volume in "${volumes[@]}"; do
  init_cmd+=( --volume "${volume}" )
done
init_cmd+=( "${machine_name}" )

write_command_file "${output_dir}/nimbus-machine-init-command.txt" "${init_cmd[@]}"
init_status=0
capture_command_allow_failure \
  "recreate.init" \
  "${output_dir}/nimbus-machine-init.txt" \
  "${init_cmd[@]}" || init_status=$?

start_status=0
if [[ "${init_status}" -eq 0 ]]; then
  start_cmd=(
    env
    "HOME=${home_dir}"
    "NIMBUS_MACHINE_RUNTIME_ROOT=${runtime_root}"
  )
  if [[ "${uses_legacy_podman_image}" -eq 1 && -n "${NIMBUS_MACHINE_GUEST_BINARY:-}" ]]; then
    start_cmd+=( "NIMBUS_MACHINE_GUEST_BINARY=${NIMBUS_MACHINE_GUEST_BINARY}" )
  fi
  if [[ -n "${NIMBUS_MACHINE_API_READY_TIMEOUT_SECS:-}" ]]; then
    start_cmd+=( "NIMBUS_MACHINE_API_READY_TIMEOUT_SECS=${NIMBUS_MACHINE_API_READY_TIMEOUT_SECS}" )
  fi
  if [[ -n "${NIMBUS_MACHINE_READY_TIMEOUT_SECS:-}" ]]; then
    start_cmd+=( "NIMBUS_MACHINE_READY_TIMEOUT_SECS=${NIMBUS_MACHINE_READY_TIMEOUT_SECS}" )
  fi
  if [[ -n "${NIMBUS_MACHINE_SSH_READY_TIMEOUT_SECS:-}" ]]; then
    start_cmd+=( "NIMBUS_MACHINE_SSH_READY_TIMEOUT_SECS=${NIMBUS_MACHINE_SSH_READY_TIMEOUT_SECS}" )
  fi
  start_cmd+=(
    "${nimbus_bin}"
    machine
    start
    "${machine_name}"
  )
  write_command_file "${output_dir}/nimbus-machine-start-command.txt" "${start_cmd[@]}"
  capture_command_allow_failure \
    "recreate.start" \
    "${output_dir}/nimbus-machine-start.txt" \
    "${start_cmd[@]}" || start_status=$?
fi

status_cmd=(
  env
  "HOME=${home_dir}"
  "NIMBUS_MACHINE_RUNTIME_ROOT=${runtime_root}"
  "${nimbus_bin}"
  machine
  status
  "${machine_name}"
)
write_command_file "${output_dir}/nimbus-machine-status-command.txt" "${status_cmd[@]}"
capture_command_allow_failure \
  "capture.final_status" \
  "${output_dir}/nimbus-machine-status.txt" \
  "${status_cmd[@]}" || true

bash "${script_dir}/collect-nimbus-machine-diagnostics.sh" \
  --machine "${machine_name}" \
  --home "${home_dir}" \
  --runtime-root "${runtime_root}" \
  --output-dir "${output_dir}/post-diagnostics" \
  --nimbus "${nimbus_bin}" \
  --log-lines "${log_lines}" \
  > "${output_dir}/post-diagnostics-stdout.txt"
print_line "capture.post_diagnostics" "ok path=${output_dir}/post-diagnostics"

if [[ "${init_status}" -ne 0 || "${start_status}" -ne 0 ]]; then
  print_line "result" "failed"
  exit 1
fi

print_line "result" "ready"
