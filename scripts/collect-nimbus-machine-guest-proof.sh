#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"

usage() {
  cat <<'EOF'
usage: collect-nimbus-machine-guest-proof.sh [options]

Collect guest-image contract proof from a booted Nimbus macOS machine using the
shipped `nimbus machine ...` surface. This is the MAC4 proof lane for:

- guest `nimbus --version`
- guest required runtime binaries
- guest `nimbus.socket` / `nimbus.service` state
- guest machine-API health/capabilities on `/run/nimbus/nimbus.sock`
- virtiofs mount presence
- host-side first-boot machine log tail

options:
  --machine <name>               Machine name (default: default)
  --home <path>                  HOME to use for XDG-style machine roots
  --runtime-root <path>          Runtime root (default: /tmp/nimbus)
  --output-dir <path>            Output directory for captured artifacts
  --nimbus <path>                Nimbus binary path
                                 (default: <repo>/target/debug/nimbus)
  --image <path>                 Optional built guest image artifact path to record
  --guest-volume-path <path>     Guest virtiofs target to prove (default: /Users)
  --guest-binary-path <path>     Guest nimbus binary path
                                 (default: /usr/local/bin/nimbus)
  --guest-socket-path <path>     Guest machine-API socket (default: /run/nimbus/nimbus.sock)
  --log-lines <count>            Number of host machine-log lines to capture
  -h, --help                     Show this help

examples:
  bash scripts/collect-nimbus-machine-guest-proof.sh \
    --home /tmp/nimbus-home \
    --runtime-root /tmp/nimbus \
    --output-dir /tmp/nimbus-machine-guest-proof \
    --nimbus target/debug/nimbus \
    --image /tmp/nimbus-machine-os/nimbus-machine-os.raw.gz
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

capture_command() {
  local label="$1"
  local command_path="$2"
  local output_path="$3"
  shift 3

  write_command_file "${command_path}" "$@"

  local status=0
  set +e
  "$@" >"${output_path}" 2>&1
  status=$?
  set -e

  if [[ "${status}" -eq 0 ]]; then
    print_line "${label}" "ok path=${output_path} cmd=${command_path}"
  else
    print_line "${label}" "failed status=${status} path=${output_path} cmd=${command_path}"
  fi

  return "${status}"
}

tail_file_if_present() {
  local label="$1"
  local source_path="$2"
  local output_path="$3"
  local line_count="$4"

  if [[ -f "${source_path}" ]]; then
    tail -n "${line_count}" "${source_path}" > "${output_path}"
    print_line "${label}" "present path=${source_path} tail=${output_path}"
  else
    print_line "${label}" "missing path=${source_path}"
  fi
}

machine_name="default"
home_dir="${HOME:-}"
runtime_root="${NIMBUS_MACHINE_RUNTIME_ROOT:-/tmp/nimbus}"
output_dir=""
nimbus_bin="${repo_root}/target/debug/nimbus"
image_artifact=""
guest_volume_path="/Users"
guest_binary_path="/usr/local/bin/nimbus"
guest_socket_path="/run/nimbus/nimbus.sock"
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
      image_artifact="${2:?missing image artifact path}"
      shift 2
      ;;
    --guest-volume-path)
      guest_volume_path="${2:?missing guest volume path}"
      shift 2
      ;;
    --guest-binary-path)
      guest_binary_path="${2:?missing guest binary path}"
      shift 2
      ;;
    --guest-socket-path)
      guest_socket_path="${2:?missing guest socket path}"
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

if [[ -z "${home_dir}" ]]; then
  echo "set --home or HOME so the machine roots can be derived" >&2
  exit 64
fi

if [[ ! -x "${nimbus_bin}" ]]; then
  echo "nimbus binary is not executable at ${nimbus_bin}; build it first or pass --nimbus" >&2
  exit 64
fi

if [[ -n "${image_artifact}" && ! -f "${image_artifact}" ]]; then
  echo "image artifact does not exist at ${image_artifact}" >&2
  exit 64
fi

if [[ -z "${output_dir}" ]]; then
  output_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-machine-guest-proof.XXXXXX")"
else
  mkdir -p "${output_dir}"
fi

output_dir="$(cd "${output_dir}" && pwd)"
summary_file="${output_dir}/summary.txt"
: > "${summary_file}"

machine_log="${runtime_root%/}/${machine_name}.log"

print_line "output.dir" "${output_dir}"
print_line "machine.name" "${machine_name}"
print_line "home.dir" "${home_dir}"
print_line "runtime.root" "${runtime_root}"
print_line "runtime.machine_log" "${machine_log}"
print_line "nimbus.bin" "${nimbus_bin}"
print_line "guest.volume_path" "${guest_volume_path}"
print_line "guest.binary_path" "${guest_binary_path}"
print_line "guest.socket_path" "${guest_socket_path}"
print_line "image.artifact" "${image_artifact:-<unspecified>}"

base_cmd=(
  env
  "HOME=${home_dir}"
  "NIMBUS_MACHINE_RUNTIME_ROOT=${runtime_root}"
  "${nimbus_bin}"
)

status_cmd=("${base_cmd[@]}" machine status)
capture_command \
  "capture.machine_status" \
  "${output_dir}/machine-status-command.txt" \
  "${output_dir}/machine-status.txt" \
  "${status_cmd[@]}" || true

ssh_base=("${base_cmd[@]}" machine ssh --)

version_cmd=("${ssh_base[@]}" "${guest_binary_path}" --version)
capture_command \
  "capture.guest_nimbus_version" \
  "${output_dir}/guest-nimbus-version-command.txt" \
  "${output_dir}/guest-nimbus-version.txt" \
  "${version_cmd[@]}" || true

version_sha_cmd=(
  "${ssh_base[@]}"
  "/bin/sh -lc 'sha256sum ${guest_binary_path}'"
)
capture_command \
  "capture.guest_nimbus_sha256" \
  "${output_dir}/guest-nimbus-sha256-command.txt" \
  "${output_dir}/guest-nimbus-sha256.txt" \
  "${version_sha_cmd[@]}" || true

runtime_bins_cmd=(
  "${ssh_base[@]}"
  "/bin/sh -lc 'helper_dirs=\"/usr/local/libexec/podman /usr/local/lib/podman /usr/libexec/podman /usr/lib/podman\"; for bin in buildah conmon crun netavark aardvark-dns fuse-overlayfs; do
     path=\"\"
     if resolved=\$(command -v \"\$bin\" 2>/dev/null); then
       path=\"\$resolved\"
     else
       for dir in \$helper_dirs; do
         candidate=\"\$dir/\$bin\"
         if [ -x \"\$candidate\" ]; then
           path=\"\$candidate\"
           break
         fi
       done
     fi
     if [ -n \"\$path\" ]; then
       printf \"present %s %s\n\" \"\$bin\" \"\$path\"
     else
       printf \"missing %s\n\" \"\$bin\"
     fi
   done'"
)
capture_command \
  "capture.guest_required_binaries" \
  "${output_dir}/guest-required-binaries-command.txt" \
  "${output_dir}/guest-required-binaries.txt" \
  "${runtime_bins_cmd[@]}" || true

socket_status_cmd=(
  "${ssh_base[@]}"
  "/bin/sh -lc 'systemctl show --no-pager --property=Id,LoadState,UnitFileState,ActiveState,SubState nimbus.socket || true'"
)
capture_command \
  "capture.guest_nimbus_socket_status" \
  "${output_dir}/guest-nimbus-socket-status-command.txt" \
  "${output_dir}/guest-nimbus-socket-status.txt" \
  "${socket_status_cmd[@]}" || true

service_status_cmd=(
  "${ssh_base[@]}"
  "/bin/sh -lc 'systemctl show --no-pager --property=Id,LoadState,UnitFileState,ActiveState,SubState nimbus.service || true'"
)
capture_command \
  "capture.guest_nimbus_service_status" \
  "${output_dir}/guest-nimbus-service-status-command.txt" \
  "${output_dir}/guest-nimbus-service-status.txt" \
  "${service_status_cmd[@]}" || true

virtiofs_cmd=(
  "${ssh_base[@]}"
  "/bin/sh -lc 'findmnt --noheadings --output TARGET,SOURCE,FSTYPE,OPTIONS -T \"${guest_volume_path}\" || stat \"${guest_volume_path}\"'"
)
capture_command \
  "capture.guest_virtiofs_mount" \
  "${output_dir}/guest-virtiofs-mount-command.txt" \
  "${output_dir}/guest-virtiofs-mount.txt" \
  "${virtiofs_cmd[@]}" || true

health_cmd=(
  "${ssh_base[@]}"
  "/bin/sh -lc 'sudo curl --silent --show-error --include --unix-socket ${guest_socket_path} http://localhost/healthz'"
)
capture_command \
  "capture.guest_machine_api_health" \
  "${output_dir}/guest-machine-api-health-command.txt" \
  "${output_dir}/guest-machine-api-health.txt" \
  "${health_cmd[@]}" || true

capabilities_cmd=(
  "${ssh_base[@]}"
  "/bin/sh -lc 'sudo curl --silent --show-error --include --unix-socket ${guest_socket_path} http://localhost/v1/machine-api/capabilities'"
)
capture_command \
  "capture.guest_machine_api_capabilities" \
  "${output_dir}/guest-machine-api-capabilities-command.txt" \
  "${output_dir}/guest-machine-api-capabilities.txt" \
  "${capabilities_cmd[@]}" || true

tail_file_if_present \
  "artifact.machine_log_tail" \
  "${machine_log}" \
  "${output_dir}/machine-log-tail.txt" \
  "${log_lines}"

print_line "result" "captured"
