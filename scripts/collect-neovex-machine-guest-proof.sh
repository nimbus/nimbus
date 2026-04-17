#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"

usage() {
  cat <<'EOF'
usage: collect-neovex-machine-guest-proof.sh [options]

Collect guest-image contract proof from a booted Neovex macOS machine using the
shipped `neovex machine ...` surface. This is the MAC4 proof lane for:

- guest `neovex --version`
- guest required runtime binaries
- guest `neovex.socket` / `neovex.service` state
- guest machine-API health/capabilities on `/run/neovex/neovex.sock`
- virtiofs mount presence
- host-side first-boot machine log tail

options:
  --machine <name>               Machine name (default: default)
  --home <path>                  HOME to use for XDG-style machine roots
  --runtime-root <path>          Runtime root (default: /tmp/neovex)
  --output-dir <path>            Output directory for captured artifacts
  --neovex <path>                Neovex binary path
                                 (default: <repo>/target/debug/neovex)
  --image <path>                 Optional built guest image artifact path to record
  --guest-volume-path <path>     Guest virtiofs target to prove (default: /Users)
  --guest-binary-path <path>     Guest neovex binary path
                                 (default: /usr/local/bin/neovex)
  --guest-socket-path <path>     Guest machine-API socket (default: /run/neovex/neovex.sock)
  --log-lines <count>            Number of host machine-log lines to capture
  -h, --help                     Show this help

examples:
  bash scripts/collect-neovex-machine-guest-proof.sh \
    --home /tmp/neovex-home \
    --runtime-root /tmp/neovex \
    --output-dir /tmp/neovex-machine-guest-proof \
    --neovex target/debug/neovex \
    --image /tmp/neovex-machine-os/neovex-machine-os.raw.gz
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
runtime_root="${NEOVEX_MACHINE_RUNTIME_ROOT:-/tmp/neovex}"
output_dir=""
neovex_bin="${repo_root}/target/debug/neovex"
image_artifact=""
guest_volume_path="/Users"
guest_binary_path="/usr/local/bin/neovex"
guest_socket_path="/run/neovex/neovex.sock"
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
    --neovex)
      neovex_bin="${2:?missing neovex path}"
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

if [[ ! -x "${neovex_bin}" ]]; then
  echo "neovex binary is not executable at ${neovex_bin}; build it first or pass --neovex" >&2
  exit 64
fi

if [[ -n "${image_artifact}" && ! -f "${image_artifact}" ]]; then
  echo "image artifact does not exist at ${image_artifact}" >&2
  exit 64
fi

if [[ -z "${output_dir}" ]]; then
  output_dir="$(mktemp -d "${TMPDIR:-/tmp}/neovex-machine-guest-proof.XXXXXX")"
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
print_line "neovex.bin" "${neovex_bin}"
print_line "guest.volume_path" "${guest_volume_path}"
print_line "guest.binary_path" "${guest_binary_path}"
print_line "guest.socket_path" "${guest_socket_path}"
print_line "image.artifact" "${image_artifact:-<unspecified>}"

base_cmd=(
  env
  "HOME=${home_dir}"
  "NEOVEX_MACHINE_RUNTIME_ROOT=${runtime_root}"
  "${neovex_bin}"
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
  "capture.guest_neovex_version" \
  "${output_dir}/guest-neovex-version-command.txt" \
  "${output_dir}/guest-neovex-version.txt" \
  "${version_cmd[@]}" || true

version_sha_cmd=(
  "${ssh_base[@]}"
  "/bin/sh -lc 'sha256sum ${guest_binary_path}'"
)
capture_command \
  "capture.guest_neovex_sha256" \
  "${output_dir}/guest-neovex-sha256-command.txt" \
  "${output_dir}/guest-neovex-sha256.txt" \
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
  "/bin/sh -lc 'systemctl show --no-pager --property=Id,LoadState,UnitFileState,ActiveState,SubState neovex.socket || true'"
)
capture_command \
  "capture.guest_neovex_socket_status" \
  "${output_dir}/guest-neovex-socket-status-command.txt" \
  "${output_dir}/guest-neovex-socket-status.txt" \
  "${socket_status_cmd[@]}" || true

service_status_cmd=(
  "${ssh_base[@]}"
  "/bin/sh -lc 'systemctl show --no-pager --property=Id,LoadState,UnitFileState,ActiveState,SubState neovex.service || true'"
)
capture_command \
  "capture.guest_neovex_service_status" \
  "${output_dir}/guest-neovex-service-status-command.txt" \
  "${output_dir}/guest-neovex-service-status.txt" \
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
  "/bin/sh -lc 'printf \"GET /healthz HTTP/1.0\\r\\nHost: localhost\\r\\n\\r\\n\" | sudo socat - UNIX-CONNECT:${guest_socket_path}'"
)
capture_command \
  "capture.guest_machine_api_health" \
  "${output_dir}/guest-machine-api-health-command.txt" \
  "${output_dir}/guest-machine-api-health.txt" \
  "${health_cmd[@]}" || true

capabilities_cmd=(
  "${ssh_base[@]}"
  "/bin/sh -lc 'printf \"GET /v1/machine-api/capabilities HTTP/1.0\\r\\nHost: localhost\\r\\n\\r\\n\" | sudo socat - UNIX-CONNECT:${guest_socket_path}'"
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
